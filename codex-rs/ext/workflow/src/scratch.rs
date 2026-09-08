//! Run-local UTF-8 scratch files under Codex home.

use std::fs;
use std::io::Write;
use std::path::Path;

use uuid::Uuid;

use crate::engine::MAX_WORKFLOW_SOURCE_CHARS;
use crate::source_read::BoundedSourceError;
use crate::source_read::read_bounded_workflow_source;

pub(crate) fn write_scratch_file(dir: &Path, name: &str, content: &str) -> Result<String, String> {
    let name = validated_scratch_name(name)?;
    if content.chars().count() > MAX_WORKFLOW_SOURCE_CHARS {
        return Err(format!(
            "scratch file exceeds {MAX_WORKFLOW_SOURCE_CHARS} characters"
        ));
    }
    reject_local_symlinks(dir)?;
    fs::create_dir_all(dir).map_err(|error| format!("scratch dir: {error}"))?;
    reject_local_symlinks(dir)?;
    let path = dir.join(name);
    let tmp = dir.join(format!(".{}.tmp", Uuid::now_v7()));
    reject_symlink(&path, "scratch file")?;
    reject_symlink(&tmp, "scratch staging file")?;
    let mut file = fs::File::create(&tmp).map_err(|error| format!("scratch write: {error}"))?;
    file.write_all(content.as_bytes())
        .map_err(|error| format!("scratch write: {error}"))?;
    drop(file);
    fs::rename(&tmp, &path).map_err(|error| format!("scratch persist: {error}"))?;
    Ok(name.to_string())
}

pub(crate) fn read_scratch_file(dir: &Path, name: &str) -> Result<String, String> {
    let name = validated_scratch_name(name)?;
    let path = dir.join(name);
    reject_local_symlinks(dir)?;
    reject_symlink(&path, "scratch file")?;
    let content = read_bounded_workflow_source(&path).map_err(|error| match error {
        BoundedSourceError::Io(error) if error.kind() == std::io::ErrorKind::NotFound => {
            "scratch file not found".to_string()
        }
        BoundedSourceError::Io(error) => format!("scratch read: {error}"),
        BoundedSourceError::TooManyBytes { .. } | BoundedSourceError::TooManyChars { .. } => {
            format!("scratch file exceeds {MAX_WORKFLOW_SOURCE_CHARS} characters")
        }
        BoundedSourceError::NotUtf8 => "scratch file must be UTF-8".to_string(),
    })?;
    Ok(content)
}

fn validated_scratch_name(name: &str) -> Result<&str, String> {
    if name.is_empty() {
        return Err("scratch file name must not be empty".to_string());
    }
    if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err("scratch file name must be a single path component".to_string());
    }
    Ok(name)
}

fn reject_local_symlinks(dir: &Path) -> Result<(), String> {
    reject_symlink(dir, "scratch directory")?;
    if let Some(parent) = dir.parent() {
        reject_symlink(parent, "scratch parent")?;
    }
    Ok(())
}

fn reject_symlink(path: &Path, what: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{what}: {error}")),
        Ok(meta) if meta.file_type().is_symlink() => Err(format!("{what} must not be a symlink")),
        Ok(_) => Ok(()),
    }
}
