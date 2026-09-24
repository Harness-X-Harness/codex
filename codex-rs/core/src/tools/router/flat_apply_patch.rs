use crate::function_tool::FunctionCallError;
use codex_apply_patch::parse_patch;

const BEGIN_PATCH_MARKER: &str = "*** Begin Patch";
const END_PATCH_MARKER: &str = "*** End Patch";

/// Reject Grok/flat-projected `apply_patch` payloads that are only string-typed
/// and do not satisfy the canonical Codex patch markers/grammar.
pub(super) fn validate_projected_apply_patch(patch: &str) -> Result<String, FunctionCallError> {
    let trimmed = patch.trim();
    let first = trimmed.lines().map(str::trim).next().unwrap_or("");
    let last = trimmed.lines().map(str::trim).next_back().unwrap_or("");
    if first != BEGIN_PATCH_MARKER {
        return Err(FunctionCallError::RespondToModel(format!(
            "apply_patch grammar rejected at the Provider boundary: expected first line \"{BEGIN_PATCH_MARKER}\", got {first:?}"
        )));
    }
    if last != END_PATCH_MARKER {
        return Err(FunctionCallError::RespondToModel(format!(
            "apply_patch grammar rejected at the Provider boundary: expected last line \"{END_PATCH_MARKER}\", got {last:?}"
        )));
    }
    parse_patch(patch).map_err(|error| {
        FunctionCallError::RespondToModel(format!(
            "apply_patch grammar rejected at the Provider boundary: {error}"
        ))
    })?;
    Ok(patch.to_string())
}
