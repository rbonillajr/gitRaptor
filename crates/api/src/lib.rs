//! JSON-RPC and event contract shared by the engine, the CLI and the MCP server.

/// Version of the engine API contract.
pub const API_VERSION: &str = "0.0.0";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_version_is_set() {
        assert!(!API_VERSION.is_empty());
    }
}
