//! The constants of the dispatchers (ADR-GRD-001 § 2, M-04): written once at install time to
//! `<common>/gitraptor/dispatch.conf`, next to the native dispatchers that read it, and hashed
//! in the journal. One `key<TAB>value` line per constant; a value with a newline, a tab, a NUL
//! or bytes that are not UTF-8 is not representable, and then nothing is installed.

use std::path::{Path, PathBuf};

/// Version of the dispatcher template: `raptor hook` accepts this one and the previous one
/// (ADR-GRD-001 § 8).
pub const TEMPLATE_VERSION: u32 = 1;

/// The constants file, next to `hooks/`.
pub const DISPATCH_CONF: &str = "dispatch.conf";

/// The manifest for the manual recovery (not authoritative, ADR-GRD-001 § 1).
pub const MANIFEST: &str = "manifest.json";

/// File name of the native dispatcher installed next to `raptor`.
pub const STUB_FILE: &str = if cfg!(windows) {
    "raptor-hook.exe"
} else {
    "raptor-hook"
};

/// Everything a dispatcher needs, fixed at install time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constants {
    pub template: u32,
    /// Stable path of the installed `raptor` (ADR-GRD-001 § 8).
    pub raptor: PathBuf,
    /// Repo id in the profile.
    pub repo: String,
    /// Canonical common directory (M-02, SEC-GRD-19).
    pub common: PathBuf,
    /// Runtime folder of the daemon's channel (H-03, SEC-GRD-16).
    pub channel: PathBuf,
    /// Profile instance id (ADR-GRD-003 § 4).
    pub instance: String,
    /// State folder of the profile: the read-only snapshot of degraded mode.
    pub state: PathBuf,
    /// Previous `core.hooksPath`, as is; empty when there was none (US-GRD-002 chains it).
    pub prior: String,
}

/// A constant that cannot be written as such.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotRepresentable(pub &'static str);

fn representable(key: &'static str, value: &str) -> Result<(), NotRepresentable> {
    if value.contains(['\n', '\r', '\t', '\0']) {
        Err(NotRepresentable(key))
    } else {
        Ok(())
    }
}

fn path_text(key: &'static str, path: &Path) -> Result<String, NotRepresentable> {
    let text = path.to_str().ok_or(NotRepresentable(key))?;
    representable(key, text)?;
    Ok(text.to_owned())
}

impl Constants {
    /// The file content, or which constant is not representable.
    pub fn render(&self) -> Result<String, NotRepresentable> {
        let mut out = String::new();
        let mut line = |key: &'static str, value: String| -> Result<(), NotRepresentable> {
            representable(key, &value)?;
            out.push_str(key);
            out.push('\t');
            out.push_str(&value);
            out.push('\n');
            Ok(())
        };
        line("template", self.template.to_string())?;
        line("raptor", path_text("raptor", &self.raptor)?)?;
        line("repo", self.repo.clone())?;
        line("common", path_text("common", &self.common)?)?;
        line("channel", path_text("channel", &self.channel)?)?;
        line("instance", self.instance.clone())?;
        line("state", path_text("state", &self.state)?)?;
        line("prior", self.prior.clone())?;
        Ok(out)
    }

    /// Parses a rendered file; `None` if anything is missing or malformed.
    pub fn parse(text: &str) -> Option<Self> {
        let mut get = std::collections::BTreeMap::new();
        for line in text.lines() {
            let (k, v) = line.split_once('\t')?;
            if get.insert(k, v).is_some() {
                return None;
            }
        }
        Some(Self {
            template: get.get("template")?.parse().ok()?,
            raptor: PathBuf::from(get.get("raptor")?),
            repo: (*get.get("repo")?).to_owned(),
            common: PathBuf::from(get.get("common")?),
            channel: PathBuf::from(get.get("channel")?),
            instance: (*get.get("instance")?).to_owned(),
            state: PathBuf::from(get.get("state")?),
            prior: (*get.get("prior")?).to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Constants {
        Constants {
            template: TEMPLATE_VERSION,
            raptor: "/usr/local/bin/raptor".into(),
            repo: "abc-123".into(),
            common: "/Users/Jos\u{e9}/r/$(x)/.git".into(),
            channel: "/run/user/1/gitraptor".into(),
            instance: "i-1".into(),
            state: "/s".into(),
            prior: String::new(),
        }
    }

    #[test]
    fn round_trips_literal_values() {
        // A `$(…)` is a literal constant: nothing interprets it (ADR-GRD-001 Validación 7).
        let c = sample();
        let text = c.render().unwrap();
        assert_eq!(Constants::parse(&text), Some(c));
    }

    #[test]
    fn control_characters_are_not_representable() {
        for bad in ["/a\nb", "/a\tb", "/a\rb", "/a\0b"] {
            let c = Constants {
                common: bad.into(),
                ..sample()
            };
            assert_eq!(c.render(), Err(NotRepresentable("common")), "{bad:?}");
        }
    }

    #[test]
    fn a_malformed_file_does_not_parse() {
        assert_eq!(Constants::parse("template\t1\n"), None);
        let text = sample().render().unwrap();
        assert_eq!(Constants::parse(&format!("{text}repo\tother\n")), None);
    }
}
