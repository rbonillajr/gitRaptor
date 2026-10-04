//! Where the profile lives on each OS (ADR-GRP-006 § 1).

use std::path::{Path, PathBuf};

use super::error::{ProfileError, Result};

/// Short, stable folder name of the profile. Kept short so the channel
/// socket path stays well under the ~104-byte limit on macOS (ADR-GRP-005).
pub const APP_DIR: &str = "gitraptor";

/// Environment variable that redirects the whole profile to one root.
/// Honored only in builds with `debug_assertions` (tests and development);
/// a release binary ignores it (SEC-06, H4).
pub const PROFILE_DIR_ENV: &str = "GITRAPTOR_PROFILE_DIR";

/// Per-repo stores live in this subfolder of the data folder.
pub(crate) const REPOS_DIR: &str = "repos";
/// Corrupt files are moved here, never deleted.
pub(crate) const QUARANTINE_DIR: &str = "quarantine";

/// The set of folders that make up the profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDirs {
    /// Engine data: global index and per-repo stores.
    pub data: PathBuf,
    /// Profile-level configuration (read-only for the engine).
    pub config: PathBuf,
    /// Instance lock and logs.
    pub state: PathBuf,
    /// Channel socket. `None` on Windows, where the channel is a named pipe.
    pub runtime: Option<PathBuf>,
    /// Every folder exclusive to GitRaptor, parents first. These are created
    /// with mode 0700 and verified on open.
    owned: Vec<PathBuf>,
}

/// The standard base folders of one OS, as reported by `directories`.
#[derive(Debug, Clone)]
pub(crate) enum BaseInputs {
    /// `~/Library/Application Support`.
    MacOs { app_support: PathBuf },
    /// XDG default folders under the home folder; `runtime` is `None` when
    /// `$XDG_RUNTIME_DIR` is unset or relative.
    Linux {
        data: PathBuf,
        config: PathBuf,
        state: PathBuf,
        runtime: Option<PathBuf>,
    },
    /// `%LOCALAPPDATA%`. The roaming folder is never an input.
    Windows { local_app_data: PathBuf },
}

impl ProfileDirs {
    /// Resolves the profile for the current user and OS.
    ///
    /// In builds with `debug_assertions`, `GITRAPTOR_PROFILE_DIR` replaces
    /// every folder with a subfolder of that root. In release builds the
    /// variable is not even read.
    pub fn resolve() -> Result<Self> {
        #[cfg(debug_assertions)]
        if let Some(root) = std::env::var_os(PROFILE_DIR_ENV).filter(|v| !v.is_empty()) {
            return Ok(Self::under_root(PathBuf::from(root)));
        }
        Ok(Self::from_base(&base_inputs()?))
    }

    /// Puts the whole profile under one root: `data/`, `config/`, `state/`
    /// and `run/`. This is how tests and embedding code inject the profile.
    pub fn under_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let data = root.join("data");
        let config = root.join("config");
        let state = root.join("state");
        let runtime = root.join("run");
        let owned = with_store_dirs(
            vec![
                root,
                data.clone(),
                config.clone(),
                state.clone(),
                runtime.clone(),
            ],
            &data,
        );
        Self {
            data,
            config,
            state,
            runtime: Some(runtime),
            owned,
        }
    }

    pub(crate) fn from_base(base: &BaseInputs) -> Self {
        match base {
            BaseInputs::MacOs { app_support } => {
                let root = app_support.join(APP_DIR);
                let (data, config, state) = split_app_root(&root);
                let owned = with_store_dirs(
                    vec![root, data.clone(), config.clone(), state.clone()],
                    &data,
                );
                Self {
                    runtime: Some(state.clone()),
                    data,
                    config,
                    state,
                    owned,
                }
            }
            BaseInputs::Linux {
                data,
                config,
                state,
                runtime,
            } => {
                let data = data.join(APP_DIR);
                let config = config.join(APP_DIR);
                let state = state.join(APP_DIR);
                let runtime = runtime
                    .as_ref()
                    .map_or_else(|| state.clone(), |r| r.join(APP_DIR));
                let mut owned = vec![data.clone(), config.clone(), state.clone()];
                if runtime != state {
                    owned.push(runtime.clone());
                }
                let owned = with_store_dirs(owned, &data);
                Self {
                    data,
                    config,
                    state,
                    runtime: Some(runtime),
                    owned,
                }
            }
            BaseInputs::Windows { local_app_data } => {
                let root = local_app_data.join(APP_DIR);
                let (data, config, state) = split_app_root(&root);
                let owned = with_store_dirs(
                    vec![root, data.clone(), config.clone(), state.clone()],
                    &data,
                );
                Self {
                    data,
                    config,
                    state,
                    runtime: None,
                    owned,
                }
            }
        }
    }

    /// Folder holding one SQLite file per repo key.
    pub fn repos_dir(&self) -> PathBuf {
        self.data.join(REPOS_DIR)
    }

    /// Folder where corrupt files are set aside.
    pub fn quarantine_dir(&self) -> PathBuf {
        self.data.join(QUARANTINE_DIR)
    }

    /// Every folder exclusive to GitRaptor, parents first.
    pub fn owned_dirs(&self) -> &[PathBuf] {
        &self.owned
    }
}

/// macOS and Windows offer a single app folder, so data, config and state
/// are split into subfolders: a quarantine of the data never reaches the
/// user's configuration (ADR-GRP-008).
fn split_app_root(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    (root.join("data"), root.join("config"), root.join("state"))
}

fn with_store_dirs(mut owned: Vec<PathBuf>, data: &Path) -> Vec<PathBuf> {
    owned.push(data.join(REPOS_DIR));
    owned.push(data.join(QUARANTINE_DIR));
    owned
}

fn base_inputs() -> Result<BaseInputs> {
    let base = directories::BaseDirs::new().ok_or(ProfileError::NoHomeDir)?;
    if cfg!(target_os = "macos") {
        Ok(BaseInputs::MacOs {
            app_support: base.data_dir().to_path_buf(),
        })
    } else if cfg!(windows) {
        // `data_local_dir` is %LOCALAPPDATA%. `config_dir`/`data_dir` would be
        // the roaming %APPDATA%, which is never used (PQ-7).
        Ok(BaseInputs::Windows {
            local_app_data: base.data_local_dir().to_path_buf(),
        })
    } else {
        // `XDG_DATA_HOME`, `XDG_CONFIG_HOME` and `XDG_STATE_HOME` are ignored:
        // an inherited hostile value would move the profile (SEC-10). Every
        // process (daemon, CLI, hook dispatcher) resolves through here, so
        // all of them agree on the same folders (ADR-GRP-006 § 1 amendment).
        let home = base.home_dir();
        Ok(BaseInputs::Linux {
            data: home.join(".local/share"),
            config: home.join(".config"),
            state: home.join(".local/state"),
            runtime: base
                .runtime_dir()
                .filter(|dir| dir.is_absolute())
                .map(Path::to_path_buf),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_adr_table_on_macos() {
        let dirs = ProfileDirs::from_base(&BaseInputs::MacOs {
            app_support: PathBuf::from("/Users/u/Library/Application Support"),
        });
        let root = PathBuf::from("/Users/u/Library/Application Support/gitraptor");
        assert_eq!(dirs.data, root.join("data"));
        assert_eq!(dirs.config, root.join("config"));
        assert_eq!(dirs.state, root.join("state"));
        assert_eq!(dirs.runtime, Some(root.join("state")));
        assert_eq!(dirs.owned_dirs()[0], root);
    }

    #[test]
    fn layout_matches_adr_table_on_linux() {
        let dirs = ProfileDirs::from_base(&BaseInputs::Linux {
            data: PathBuf::from("/home/u/.local/share"),
            config: PathBuf::from("/home/u/.config"),
            state: PathBuf::from("/home/u/.local/state"),
            runtime: Some(PathBuf::from("/run/user/1000")),
        });
        assert_eq!(dirs.data, PathBuf::from("/home/u/.local/share/gitraptor"));
        assert_eq!(dirs.config, PathBuf::from("/home/u/.config/gitraptor"));
        assert_eq!(dirs.state, PathBuf::from("/home/u/.local/state/gitraptor"));
        assert_eq!(
            dirs.runtime,
            Some(PathBuf::from("/run/user/1000/gitraptor"))
        );
        assert!(
            dirs.owned_dirs()
                .contains(&PathBuf::from("/run/user/1000/gitraptor"))
        );
    }

    #[test]
    fn linux_runtime_falls_back_to_state() {
        let dirs = ProfileDirs::from_base(&BaseInputs::Linux {
            data: PathBuf::from("/d"),
            config: PathBuf::from("/c"),
            state: PathBuf::from("/s"),
            runtime: None,
        });
        assert_eq!(dirs.runtime, Some(PathBuf::from("/s/gitraptor")));
    }

    #[test]
    fn layout_matches_adr_table_on_windows() {
        let local = PathBuf::from(r"C:\Users\u\AppData\Local");
        let dirs = ProfileDirs::from_base(&BaseInputs::Windows {
            local_app_data: local.clone(),
        });
        let root = local.join("gitraptor");
        assert_eq!(dirs.data, root.join("data"));
        assert_eq!(dirs.config, root.join("config"));
        assert_eq!(dirs.state, root.join("state"));
        assert_eq!(dirs.runtime, None);
        for dir in dirs.owned_dirs() {
            assert!(
                dir.starts_with(&local),
                "{} outside LOCALAPPDATA",
                dir.display()
            );
        }
    }

    #[test]
    fn data_and_config_never_share_a_folder() {
        for base in [
            BaseInputs::MacOs {
                app_support: PathBuf::from("/a"),
            },
            BaseInputs::Windows {
                local_app_data: PathBuf::from("/w"),
            },
        ] {
            let dirs = ProfileDirs::from_base(&base);
            assert!(!dirs.config.starts_with(&dirs.data));
            assert!(!dirs.data.starts_with(&dirs.config));
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_uses_local_appdata_only() {
        let base = directories::BaseDirs::new().unwrap();
        let dirs = ProfileDirs::from_base(&base_inputs().unwrap());
        let roaming = base.config_dir();
        for dir in dirs.owned_dirs() {
            assert!(
                !dir.starts_with(roaming),
                "{} is under roaming",
                dir.display()
            );
        }
    }

    #[test]
    fn real_layout_resolves_without_creating_anything() {
        // Resolution only computes paths; it must never touch the real profile.
        let dirs = ProfileDirs::from_base(&base_inputs().unwrap());
        assert!(dirs.data.ends_with("data") || dirs.data.ends_with(APP_DIR));
    }

    #[test]
    fn under_root_puts_everything_below_root() {
        let dirs = ProfileDirs::under_root("/tmp/p");
        for dir in dirs.owned_dirs() {
            assert!(dir.starts_with("/tmp/p"));
        }
        assert_eq!(dirs.repos_dir(), PathBuf::from("/tmp/p/data/repos"));
    }
}
