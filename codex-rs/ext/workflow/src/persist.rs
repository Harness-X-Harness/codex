//! Bounded atomic JSON persistence for one Workflow run document.

use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
use std::path::Path;

use crate::engine::MAX_WORKFLOW_REPLY_CHARS;
use crate::engine::MAX_WORKFLOW_SOURCE_CHARS;
use crate::engine::MAX_WORKFLOW_YIELDS;

/// Inclusive persisted-run byte cap derived from source, result, args, and
/// journal/reply framing. UTF-8 worst case is four bytes per character.
pub const MAX_WORKFLOW_PERSIST_BYTES: usize = MAX_WORKFLOW_SOURCE_CHARS
    .saturating_mul(4)
    .saturating_mul(3)
    .saturating_add(
        MAX_WORKFLOW_REPLY_CHARS
            .saturating_mul(4)
            .saturating_mul(MAX_WORKFLOW_YIELDS as usize),
    )
    .saturating_add(64 * 1024);

/// Why a Workflow persist read or atomic replace failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistError {
    TooLarge { actual: u64 },
    UnsafePath(String),
    Io(String),
}

impl std::fmt::Display for PersistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { actual } => write!(
                f,
                "workflow persist is {actual} bytes; max is {MAX_WORKFLOW_PERSIST_BYTES}"
            ),
            Self::UnsafePath(reason) | Self::Io(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for PersistError {}

/// Atomically replace `{thread_id}.json` with `body`.
pub fn persist_workflow_document(
    persist_root: &Path,
    thread_id: &str,
    body: &[u8],
) -> Result<(), PersistError> {
    if body.len() > MAX_WORKFLOW_PERSIST_BYTES {
        return Err(PersistError::TooLarge {
            actual: body.len() as u64,
        });
    }
    reject_symlink(persist_root, "workflow persist directory")?;
    fs::create_dir_all(persist_root).map_err(|error| PersistError::Io(error.to_string()))?;
    reject_symlink(persist_root, "workflow persist directory")?;

    let final_path = persist_root.join(format!("{thread_id}.json"));
    let staging = persist_root.join(format!("{thread_id}.json.tmp"));
    reject_symlink(&final_path, "workflow persist file")?;
    prepare_staging(&staging)?;

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging)
        .map_err(|error| PersistError::Io(error.to_string()))?;
    file.write_all(body)
        .map_err(|error| PersistError::Io(error.to_string()))?;
    file.sync_all()
        .map_err(|error| PersistError::Io(error.to_string()))?;
    drop(file);

    reject_symlink(&final_path, "workflow persist file")?;
    if let Err(error) = fs::rename(&staging, &final_path) {
        let _ = fs::remove_file(&staging);
        return Err(PersistError::Io(error.to_string()));
    }
    Ok(())
}

/// Read `{thread_id}.json` if present, without following a symlink or reading
/// past [`MAX_WORKFLOW_PERSIST_BYTES`].
pub fn load_workflow_document(
    persist_root: &Path,
    thread_id: &str,
) -> Result<Option<Vec<u8>>, PersistError> {
    let path = persist_root.join(format!("{thread_id}.json"));
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(PersistError::Io(error.to_string())),
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(PersistError::UnsafePath(
            "workflow persist file must be a non-symlink regular file".to_string(),
        ));
    }
    if meta.len() > MAX_WORKFLOW_PERSIST_BYTES as u64 {
        return Err(PersistError::TooLarge { actual: meta.len() });
    }

    let file = File::open(&path).map_err(|error| PersistError::Io(error.to_string()))?;
    let mut buf = Vec::new();
    let read = file
        .take(MAX_WORKFLOW_PERSIST_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|error| PersistError::Io(error.to_string()))?;
    if read > MAX_WORKFLOW_PERSIST_BYTES {
        return Err(PersistError::TooLarge {
            actual: read as u64,
        });
    }
    Ok(Some(buf))
}

fn prepare_staging(staging: &Path) -> Result<(), PersistError> {
    match fs::symlink_metadata(staging) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PersistError::Io(error.to_string())),
        Ok(meta) if meta.file_type().is_symlink() => Err(PersistError::UnsafePath(
            "workflow persist staging file must not be a symlink".to_string(),
        )),
        Ok(_) => {
            fs::remove_file(staging).map_err(|error| PersistError::Io(error.to_string()))?;
            Ok(())
        }
    }
}

fn reject_symlink(path: &Path, what: &str) -> Result<(), PersistError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PersistError::Io(error.to_string())),
        Ok(meta) if meta.file_type().is_symlink() => Err(PersistError::UnsafePath(format!(
            "{what} must not be a symlink"
        ))),
        Ok(_) => Ok(()),
    }
}
