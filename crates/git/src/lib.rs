//! Git layer: gitoxide for reads, the system Git CLI for writes (respects hooks, config and credentials).

/// Name of the Git executable used for write operations.
pub const GIT_BIN: &str = "git";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_bin_is_git() {
        assert_eq!(GIT_BIN, "git");
    }
}
