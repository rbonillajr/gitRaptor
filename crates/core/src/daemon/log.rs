//! Daemon log in the state folder of the profile (ADR-GRP-005 § 4, SEC-05).
//!
//! Redaction is enforced by types, not by filtering: an event name is a
//! `&'static str` and every value is a [`Field`], which can only hold
//! numbers, booleans, static text, Git versions and opaque ids. There is no
//! way to log a repo path, file content, a config value, the environment or
//! the argv of another program. The panic hook records only where the panic
//! happened, never its message.
//!
//! One line per entry: `<utc_ms> <LEVEL> <event> key=value ...`. The file is
//! 0600 and rotates by size: `daemon.log` → `daemon.log.1` → ... up to
//! `keep` old files.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gitraptor_git::GitVersion;

use crate::profile::fsperm;

/// File name of the current log inside the state folder.
pub const LOG_FILE: &str = "daemon.log";

/// Size limit and number of rotated files kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogLimits {
    /// The current file rotates before it would exceed this size.
    pub max_bytes: u64,
    /// Rotated files kept (`daemon.log.1` ... `daemon.log.<keep>`).
    pub keep: usize,
}

impl Default for LogLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1024 * 1024,
            keep: 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

/// A value that may appear in the log. Deliberately closed: free text from
/// the user, a repo, the environment or another process cannot be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    Int(i64),
    Bool(bool),
    Text(&'static str),
    Version(GitVersion),
    /// Opaque id (repo key, instance id): only hex digits and dashes.
    Id(String),
    /// Source location of a panic: only `[A-Za-z0-9_./-]`.
    Location(String),
}

impl Field {
    /// An opaque id. Anything that is not hex digits and dashes (up to 64
    /// characters) is replaced by `invalid-id`.
    pub fn id(value: &str) -> Self {
        let ok = !value.is_empty()
            && value.len() <= 64
            && value.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        Self::Id(if ok {
            value.to_owned()
        } else {
            "invalid-id".into()
        })
    }

    /// Only the file name: the full path may hold the build machine's
    /// home folder (dependencies live under `~/.cargo`).
    fn location(file: &str, line: u32) -> Self {
        let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
        let file: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || "_./-".contains(c) {
                    c
                } else {
                    '_'
                }
            })
            .take(200)
            .collect();
        Self::Location([file, line.to_string()].join(":"))
    }

    fn write_to(&self, out: &mut String) {
        match self {
            Self::Int(n) => out.push_str(&n.to_string()),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Text(t) => out.push_str(t),
            Self::Version(v) => out.push_str(&v.to_string()),
            Self::Id(s) | Self::Location(s) => out.push_str(s),
        }
    }
}

impl From<i64> for Field {
    fn from(n: i64) -> Self {
        Self::Int(n)
    }
}

impl From<usize> for Field {
    fn from(n: usize) -> Self {
        Self::Int(i64::try_from(n).unwrap_or(i64::MAX))
    }
}

impl From<u32> for Field {
    fn from(n: u32) -> Self {
        Self::Int(n.into())
    }
}

impl From<bool> for Field {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl From<&'static str> for Field {
    fn from(t: &'static str) -> Self {
        Self::Text(t)
    }
}

impl From<GitVersion> for Field {
    fn from(v: GitVersion) -> Self {
        Self::Version(v)
    }
}

/// Shared handle to the daemon log. Cloning shares the same file.
#[derive(Clone)]
pub struct Logger {
    inner: Arc<Mutex<LogFile>>,
}

struct LogFile {
    dir: PathBuf,
    file: File,
    written: u64,
    limits: LogLimits,
}

impl Logger {
    /// Opens (or creates, 0600) `daemon.log` in `state_dir` for appending.
    pub fn open(state_dir: &Path, limits: LogLimits) -> io::Result<Self> {
        let path = state_dir.join(LOG_FILE);
        let file = open_append(&path)?;
        let written = file.metadata()?.len();
        Ok(Self {
            inner: Arc::new(Mutex::new(LogFile {
                dir: state_dir.to_path_buf(),
                file,
                written,
                limits,
            })),
        })
    }

    pub fn info(&self, event: &'static str, fields: &[(&'static str, Field)]) {
        self.log(Level::Info, event, fields);
    }

    pub fn warn(&self, event: &'static str, fields: &[(&'static str, Field)]) {
        self.log(Level::Warn, event, fields);
    }

    pub fn error(&self, event: &'static str, fields: &[(&'static str, Field)]) {
        self.log(Level::Error, event, fields);
    }

    /// Writes one line. A logging failure never stops the daemon.
    pub fn log(&self, level: Level, event: &'static str, fields: &[(&'static str, Field)]) {
        let mut line = String::with_capacity(96);
        line.push_str(&super::now_ms().to_string());
        line.push(' ');
        line.push_str(level.as_str());
        line.push(' ');
        line.push_str(event);
        for (key, value) in fields {
            line.push(' ');
            line.push_str(key);
            line.push('=');
            value.write_to(&mut line);
        }
        line.push('\n');
        let mut inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let _ = inner.write_line(line.as_bytes());
    }

    /// Flushes the file to disk.
    pub fn flush(&self) {
        if let Ok(inner) = self.inner.lock() {
            let _ = inner.file.sync_all();
        }
    }

    /// Replaces the panic hook with one that logs only the location of the
    /// panic: its message may carry anything (SEC-05). Process-wide, so only
    /// the `raptor daemon` entry point installs it.
    pub fn install_panic_hook(&self) {
        let logger = self.clone();
        std::panic::set_hook(Box::new(move |info| {
            let location = info.location().map_or_else(
                || Field::Text("unknown"),
                |l| Field::location(l.file(), l.line()),
            );
            logger.error("panic", &[("at", location)]);
            logger.flush();
            let _ = writeln!(
                io::stderr(),
                "raptor daemon: internal error (see daemon.log)"
            );
        }));
    }
}

impl LogFile {
    fn write_line(&mut self, line: &[u8]) -> io::Result<()> {
        let len = line.len() as u64;
        if self.written > 0 && self.written + len > self.limits.max_bytes {
            self.rotate()?;
        }
        self.file.write_all(line)?;
        self.written += len;
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        let name = |n: usize| self.dir.join([LOG_FILE, ".", &n.to_string()].concat());
        if self.limits.keep == 0 {
            fs::remove_file(self.dir.join(LOG_FILE))?;
        } else {
            let _ = fs::remove_file(name(self.limits.keep));
            for n in (1..self.limits.keep).rev() {
                let from = name(n);
                if from.exists() {
                    fs::rename(&from, name(n + 1))?;
                }
            }
            fs::rename(self.dir.join(LOG_FILE), name(1))?;
        }
        self.file = open_append(&self.dir.join(LOG_FILE))?;
        self.written = 0;
        Ok(())
    }
}

/// Appends to `path`, creating it 0600; never through a symlink.
fn open_append(path: &Path) -> io::Result<File> {
    match fsperm::create_private_file(path) {
        Ok(file) => Ok(file),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            let mut options = OpenOptions::new();
            options.append(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
            }
            let file = options.open(path)?;
            if !file.metadata()?.is_file() {
                return Err(io::Error::other("daemon.log is not a regular file"));
            }
            Ok(file)
        }
        Err(err) => Err(err),
    }
    .map(|mut file| {
        use std::io::Seek;
        let _ = file.seek(io::SeekFrom::End(0));
        file
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_locations_cannot_carry_free_text() {
        assert_eq!(
            Field::id("3f2a-00ff"),
            Field::Id("3f2a-00ff".into()),
            "hex ids pass"
        );
        assert_eq!(Field::id("/home/u/secret"), Field::Id("invalid-id".into()));
        assert_eq!(Field::id("TOKEN=abc"), Field::Id("invalid-id".into()));
        assert_eq!(
            Field::location("/home/u/.cargo/src/a b$(x).rs", 7),
            Field::Location("a_b__x_.rs:7".into())
        );
    }

    #[test]
    fn rotates_by_size_and_keeps_a_bounded_number_of_files() {
        let tmp = tempfile::tempdir().unwrap();
        let limits = LogLimits {
            max_bytes: 200,
            keep: 2,
        };
        let logger = Logger::open(tmp.path(), limits).unwrap();
        for n in 0..100_i64 {
            logger.info("tick", &[("n", n.into())]);
        }
        let current = fs::metadata(tmp.path().join(LOG_FILE)).unwrap().len();
        assert!(current <= 200, "current log is {current} bytes");
        assert!(tmp.path().join("daemon.log.1").exists());
        assert!(tmp.path().join("daemon.log.2").exists());
        assert!(!tmp.path().join("daemon.log.3").exists());
    }

    #[cfg(unix)]
    #[test]
    fn log_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let logger = Logger::open(
            tmp.path(),
            LogLimits {
                max_bytes: 64,
                keep: 1,
            },
        )
        .unwrap();
        for _ in 0..5 {
            logger.info("tick", &[]);
        }
        for name in [LOG_FILE, "daemon.log.1"] {
            let mode = fs::metadata(tmp.path().join(name))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{name}");
        }
    }
}
