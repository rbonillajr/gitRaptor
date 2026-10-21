//! The two uses of the system Git: alternative B (`merge-tree` in a store of the profile with
//! `objects/info/alternates` towards the repo) and the fidelity reference (`merge-tree` in a
//! disposable clone). Fixed argv, no shell, empty environment, isolated configuration.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::hunks;

pub struct Git {
    pub bin: PathBuf,
    pub home: PathBuf,
}

impl Git {
    pub fn new(bin: impl Into<PathBuf>, home: impl Into<PathBuf>) -> Self {
        Self {
            bin: bin.into(),
            home: home.into(),
        }
    }

    pub fn cmd(&self, git_dir: &Path) -> Command {
        let mut c = Command::new(&self.bin);
        c.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .arg("--git-dir")
            .arg(git_dir)
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "protocol.allow=never",
                "-c",
                "merge.renames=true",
                "-c",
                "merge.renameLimit=1000",
                "-c",
                "diff.renameLimit=1000",
                "-c",
                "merge.conflictStyle=merge",
                "-c",
                "core.attributesFile=/dev/null",
            ])
            .stdin(Stdio::null());
        c
    }

    pub fn version(&self) -> (u32, u32) {
        let out = Command::new(&self.bin).arg("--version").output().ok();
        let s = out
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        let v: Vec<u32> = s
            .split_whitespace()
            .nth(2)
            .unwrap_or("0.0")
            .split('.')
            .take(2)
            .filter_map(|p| p.parse().ok())
            .collect();
        (
            v.first().copied().unwrap_or(0),
            v.get(1).copied().unwrap_or(0),
        )
    }
}

#[derive(Debug, Default)]
pub struct MergeTree {
    pub conflicts: Vec<String>,
    pub hunks: Vec<(String, Vec<[u32; 4]>)>,
    pub tree: String,
}

/// `git [<global>] merge-tree --write-tree -z <ours> <theirs>` (no `--name-only`: the stages give the
/// blob ids of each side for E7). Exit 0 = clean, 1 = conflicts, anything else = error.
pub fn merge_tree(
    git: &Git,
    git_dir: &Path,
    ours: &str,
    theirs: &str,
    global: &[&str],
    with_hunks: bool,
    marker: u8,
) -> Result<MergeTree, String> {
    let out = git
        .cmd(git_dir)
        .args(global)
        .args(["merge-tree", "--write-tree", "-z"])
        .args([ours, theirs])
        .output()
        .map_err(|e| format!("spawn git: {e}"))?;
    let exit = out.status.code().unwrap_or(-1);
    if exit != 0 && exit != 1 {
        return Err(format!(
            "merge-tree exit {exit}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let mut parts = out.stdout.split(|b| *b == 0);
    let tree = String::from_utf8_lossy(parts.next().unwrap_or_default()).into_owned();
    let mut r = MergeTree {
        tree,

        ..Default::default()
    };
    // "<mode> <oid> <stage>\t<path>" until an empty record.
    let mut stages: std::collections::BTreeMap<String, [Option<String>; 4]> = Default::default();
    for rec in parts {
        if rec.is_empty() {
            break;
        }
        let s = String::from_utf8_lossy(rec);
        let Some((meta, path)) = s.split_once('\t') else {
            continue;
        };
        let f: Vec<&str> = meta.split(' ').collect();
        if f.len() != 3 {
            continue;
        }
        let stage: usize = f[2].parse().unwrap_or(0);
        let e = stages.entry(path.to_owned()).or_default();
        if stage < 4 {
            e[stage] = Some(f[1].to_owned());
        }
    }
    r.conflicts = stages.keys().cloned().collect();
    if with_hunks {
        for (path, st) in &stages {
            let (Some(o), Some(t)) = (&st[2], &st[3]) else {
                continue;
            };
            let merged = cat(git, git_dir, &format!("{}:{}", r.tree, path));
            let (Ok(m), Ok(ob), Ok(tb)) = (merged, cat(git, git_dir, o), cat(git, git_dir, t))
            else {
                continue;
            };
            if !m
                .windows(marker as usize)
                .any(|w| w.iter().all(|b| *b == b'<'))
            {
                continue; // binary or auto-resolved content: no text conflict
            }
            if let Some(h) = hunks::side_ranges(&m, &ob, &tb, marker) {
                r.hunks.push((path.clone(), h));
            }
        }
    }
    Ok(r)
}

fn cat(git: &Git, git_dir: &Path, rev: &str) -> Result<Vec<u8>, String> {
    let out = git
        .cmd(git_dir)
        .args(["cat-file", "blob", rev])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// A store in the profile for alternative B: a bare repo whose only link to the observed repo
/// is `objects/info/alternates`.
pub fn make_store(git: &Git, store: &Path, repo_objects: &Path) -> Result<(), String> {
    let st = Command::new(&git.bin)
        .env_clear()
        .env("HOME", &git.home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(["init", "-q", "--bare"])
        .arg(store)
        .status()
        .map_err(|e| e.to_string())?;
    if !st.success() {
        return Err("git init --bare failed".into());
    }
    std::fs::write(
        store.join("objects/info/alternates"),
        format!("{}\n", repo_objects.display()),
    )
    .map_err(|e| e.to_string())
}

/// A disposable full copy for the reference, so `merge-tree` writes nowhere that matters.
pub fn disposable_clone(git: &Git, src: &Path, dst: &Path, marker: u8) -> Result<(), String> {
    let st = Command::new(&git.bin)
        .env_clear()
        .env("HOME", &git.home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(["clone", "-q", "--mirror", "--no-local"])
        .arg(src)
        .arg(dst)
        .status()
        .map_err(|e| e.to_string())?;
    if !st.success() {
        return Err("git clone --mirror failed".into());
    }
    std::fs::create_dir_all(dst.join("info")).map_err(|e| e.to_string())?;
    std::fs::write(
        dst.join("info/attributes"),
        format!("* conflict-marker-size={marker}\n"),
    )
    .map_err(|e| e.to_string())
}
