//! Facts about the system read from the kernel, never from the environment
//! (whoever launches a process chooses its environment).

use std::path::PathBuf;
use std::sync::OnceLock;

/// The shared Windows folder (`C:\\Windows`), read once.
pub fn windows_dir() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(crate::ffi_process::windows_dir).as_ref()
}

/// Whether the Windows session `id` is interactive: not session 0 (services, OpenSSH) and
/// with a user connected, at the physical console or over Remote Desktop
/// (DS-TS-GRP-004 § 9, C3). Any read failure answers no.
pub fn session_is_interactive(id: u32) -> bool {
    id != 0 && crate::ffi_process::session_active(id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_windows_folder_holds_explorer() {
        let dir = super::windows_dir().unwrap();
        assert!(dir.is_absolute());
        assert!(dir.join("explorer.exe").is_file());
    }

    #[test]
    fn session_zero_is_not_an_interactive_console() {
        assert!(!super::session_is_interactive(0));
        // A session that does not exist is not interactive either.
        assert!(!super::session_is_interactive(u32::MAX - 1));
    }
}
