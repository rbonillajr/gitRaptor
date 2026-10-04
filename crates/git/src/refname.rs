//! Validated reference names (SEC-02).
//!
//! A ref that comes from a repository, a config file or a client is never trusted: it must follow
//! the `git check-ref-format` rules and must not look like an option.

use crate::ReadError;

/// A reference name that passed the `check-ref-format` rules, such as `main`,
/// `refs/heads/feature/x` or `origin/main`. It never starts with `-`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RefName(String);

impl RefName {
    /// Validate `name`.
    pub fn new(name: &str) -> Result<Self, ReadError> {
        let invalid = |why: &str| Err(ReadError::InvalidInput(format!("invalid ref name: {why}")));
        if name.is_empty() {
            return invalid("empty");
        }
        if name.starts_with('-') {
            return invalid("starts with '-'");
        }
        if name == "@" {
            return invalid("'@' alone");
        }
        if gix::validate::reference::name_partial(name.into()).is_err() {
            return invalid("check-ref-format rules");
        }
        Ok(Self(name.to_owned()))
    }

    /// The validated name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RefName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_regular_names() {
        for name in [
            "main",
            "feature/x",
            "refs/heads/main",
            "origin/main",
            "HEAD",
            "v1.0",
        ] {
            assert!(RefName::new(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn rejects_option_like_and_malformed_names() {
        for name in [
            "--upload-pack=x",
            "-x",
            "",
            "a..b",
            "a b",
            "a~1",
            "a^",
            "a:b",
            "a?",
            "a*",
            "a[",
            "a\\b",
            "a.lock",
            "a/",
            "a@{1}",
            "@",
            "a\nb",
            "/a",
        ] {
            assert!(RefName::new(name).is_err(), "{name:?}");
        }
    }
}
