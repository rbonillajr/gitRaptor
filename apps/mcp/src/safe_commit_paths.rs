//! The lexical check of the paths of `safe_commit`, before the engine is asked: too many paths,
//! or a path whose form is not a literal file of the worktree, is refused here with its code.
//! What the form cannot tell (links, ignored files, nested repos) is the daemon's check.

use gitraptor_api::catalog::MAX_COMMIT_PATHS;
use gitraptor_api::commit_paths::{PathRefusal, check_commit_paths};
use gitraptor_api::mcp_view::{MAX_MCP_PATH_BYTES, McpToolError};
use serde_json::{Value, json};

use crate::messages::ToolRefusal;

/// `Ok` for no paths or paths of a valid form. More than [`MAX_COMMIT_PATHS`] is
/// `too-many-paths` before any path is looked at; otherwise the first bad path decides, and
/// travels in `params.path` as untrusted text cut at [`MAX_MCP_PATH_BYTES`].
pub fn check(paths: Option<&[String]>) -> Result<(), ToolRefusal> {
    let Some(paths) = paths else {
        return Ok(());
    };
    let (index, refused) = match check_commit_paths(paths) {
        Ok(()) => return Ok(()),
        Err(refused) => refused,
    };
    let code = match refused {
        PathRefusal::TooMany => {
            return Err(ToolRefusal {
                code: McpToolError::TooManyPaths,
                params: Some(json!({"max": MAX_COMMIT_PATHS})),
            });
        }
        PathRefusal::Invalid => McpToolError::InvalidPath,
        PathRefusal::Outside => McpToolError::PathOutsideWorktree,
    };
    Err(ToolRefusal {
        code,
        params: paths.get(index).map(|path| path_params(path)),
    })
}

/// `{"path": {"untrusted": …}}` with the head of `path`, at most [`MAX_MCP_PATH_BYTES`].
pub(crate) fn path_params(path: &str) -> Value {
    let mut end = path.len().min(MAX_MCP_PATH_BYTES);
    while !path.is_char_boundary(end) {
        end -= 1;
    }
    json!({"path": {"untrusted": &path[..end]}})
}

#[cfg(test)]
#[path = "safe_commit_paths_tests.rs"]
mod tests;
