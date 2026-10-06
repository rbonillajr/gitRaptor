//! String values under `HKEY_CURRENT_USER`, for the login autostart (the
//! `Run` value of US-GRP-004, ADR-GRP-005 § 3). The subkey is created when
//! a value is set; it is never deleted.

use std::io;
use std::path::Path;

use crate::ffi_registry;

/// The string value `name` of `HKCU\<subkey>`, `None` if it is absent.
pub fn get_string(subkey: &Path, name: &str) -> io::Result<Option<String>> {
    ffi_registry::get(subkey, name)
}

/// Sets the string value `name` of `HKCU\<subkey>`.
pub fn set_string(subkey: &Path, name: &str, value: &str) -> io::Result<()> {
    ffi_registry::set(subkey, name, value)
}

/// Deletes the value `name` of `HKCU\<subkey>`; `false` if it was absent.
pub fn delete_value(subkey: &Path, name: &str) -> io::Result<bool> {
    ffi_registry::delete(subkey, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A subkey of its own per run: the real `Run` key is never touched.
    #[test]
    fn a_value_is_set_read_and_deleted() {
        let subkey = format!(r"Software\GitRaptorTest\registry-{}", std::process::id());
        let subkey = Path::new(&subkey);
        assert_eq!(get_string(subkey, "GitRaptor").unwrap(), None);
        let value = r#""C:\Program Files\GitRaptor\raptor.exe" daemon --autostart"#;
        set_string(subkey, "GitRaptor", value).unwrap();
        assert_eq!(
            get_string(subkey, "GitRaptor").unwrap().as_deref(),
            Some(value)
        );
        assert!(delete_value(subkey, "GitRaptor").unwrap());
        assert!(!delete_value(subkey, "GitRaptor").unwrap());
        assert_eq!(get_string(subkey, "GitRaptor").unwrap(), None);
    }
}
