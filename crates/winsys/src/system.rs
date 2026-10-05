//! Facts about the system read from the kernel, never from the environment
//! (whoever launches a process chooses its environment).

use std::path::PathBuf;
use std::sync::OnceLock;

/// The shared Windows folder (`C:\\Windows`), read once.
pub fn windows_dir() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(crate::ffi::windows_dir).as_ref()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_windows_folder_holds_explorer() {
        let dir = super::windows_dir().unwrap();
        assert!(dir.is_absolute());
        assert!(dir.join("explorer.exe").is_file());
    }
}
