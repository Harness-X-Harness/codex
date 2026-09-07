//! Bounded `/workflow start <path>` reads. Caps stay aligned with the
//! Workflow catalog/scratch reader.

use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Keep aligned with `codex_workflow_extension::MAX_WORKFLOW_SOURCE_CHARS`.
const MAX_WORKFLOW_SOURCE_CHARS: usize = 32_000;
/// Keep aligned with `codex_workflow_extension::MAX_WORKFLOW_SOURCE_BYTES`.
const MAX_WORKFLOW_SOURCE_BYTES: usize = MAX_WORKFLOW_SOURCE_CHARS * 4 + 4;

pub(crate) fn read_workflow_path_source(path: &Path) -> Result<String, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("expected a non-symlink regular file".to_string());
    }
    let file_len = meta.len();
    if file_len > MAX_WORKFLOW_SOURCE_BYTES as u64 {
        return Err(format!(
            "workflow source is {file_len} bytes; max is {MAX_WORKFLOW_SOURCE_BYTES}"
        ));
    }
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut buf = Vec::new();
    let read = file
        .take(MAX_WORKFLOW_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|error| error.to_string())?;
    if read > MAX_WORKFLOW_SOURCE_BYTES {
        return Err(format!(
            "workflow source is {read} bytes; max is {MAX_WORKFLOW_SOURCE_BYTES}"
        ));
    }
    let source = String::from_utf8(buf).map_err(|_| "workflow source must be UTF-8".to_string())?;
    let chars = source.chars().count();
    if chars > MAX_WORKFLOW_SOURCE_CHARS {
        return Err(format!(
            "workflow source is {chars} characters; max is {MAX_WORKFLOW_SOURCE_CHARS}"
        ));
    }
    Ok(source)
}

#[cfg(test)]
#[path = "workflow_source_tests.rs"]
mod tests;
