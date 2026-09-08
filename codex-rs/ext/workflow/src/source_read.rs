//! Bounded UTF-8 reads for Workflow source and scratch files.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::engine::MAX_WORKFLOW_SOURCE_BYTES;
use crate::engine::MAX_WORKFLOW_SOURCE_CHARS;

/// Why a bounded Workflow text read failed.
#[derive(Debug)]
pub(crate) enum BoundedSourceError {
    Io(std::io::Error),
    TooManyBytes { actual: u64 },
    NotUtf8,
    TooManyChars { actual: usize },
}

/// Read a non-symlink regular file using the Workflow source byte/char caps.
///
/// Metadata size is checked before allocation. The read itself is capped so a
/// file that grows after `stat` cannot become an unbounded allocation.
pub(crate) fn read_bounded_workflow_source(path: &Path) -> Result<String, BoundedSourceError> {
    let meta = std::fs::symlink_metadata(path).map_err(BoundedSourceError::Io)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(BoundedSourceError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "expected a non-symlink regular file",
        )));
    }
    let file_len = meta.len();
    if file_len > MAX_WORKFLOW_SOURCE_BYTES as u64 {
        return Err(BoundedSourceError::TooManyBytes { actual: file_len });
    }

    let file = File::open(path).map_err(BoundedSourceError::Io)?;
    let mut buf = Vec::new();
    let limit = MAX_WORKFLOW_SOURCE_BYTES as u64 + 1;
    let read = file
        .take(limit)
        .read_to_end(&mut buf)
        .map_err(BoundedSourceError::Io)?;
    if read > MAX_WORKFLOW_SOURCE_BYTES {
        return Err(BoundedSourceError::TooManyBytes {
            actual: read as u64,
        });
    }

    let source = String::from_utf8(buf).map_err(|_| BoundedSourceError::NotUtf8)?;
    let chars = source.chars().count();
    if chars > MAX_WORKFLOW_SOURCE_CHARS {
        return Err(BoundedSourceError::TooManyChars { actual: chars });
    }
    Ok(source)
}
