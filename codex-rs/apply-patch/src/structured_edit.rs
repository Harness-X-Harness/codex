//! Exact-match replacements for the structured file-edit tool.
//!
//! Matching is a UTF-8 substring search. Cardinality failures return without a
//! replacement so callers can refuse to mutate the file.

use std::path::PathBuf;

use anyhow::Context;
use codex_exec_server::ExecutorFileSystem;
use codex_exec_server::FileSystemSandboxContext;
use codex_exec_server::ReadFileOptions;
use codex_exec_server::WriteFileOptions;
use thiserror::Error;

use crate::AffectedPaths;
use crate::AppliedPatchChange;
use crate::AppliedPatchDelta;
use crate::AppliedPatchFileChange;
use crate::ApplyPatchAction;
use crate::ApplyPatchError;
use crate::ApplyPatchFailure;
use crate::ApplyPatchFileChange;
use crate::ApplyPatchOptions;
use crate::IoError;
use crate::print_summary;

/// Why an exact-match replacement was rejected before any write.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum StructuredEditError {
    #[error("old_string must be non-empty")]
    EmptyOldString,
    #[error("old_string and new_string must differ")]
    OldEqualsNew,
    #[error("old_string was not found in the file")]
    ZeroMatches,
    #[error("old_string matched {count} times; set replace_all=true to replace every match")]
    MultipleMatches { count: usize },
}

/// Apply an exact UTF-8 replacement according to the structured-edit cardinality rules.
pub fn apply_exact_replacement(
    content: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<String, StructuredEditError> {
    if old_string.is_empty() {
        return Err(StructuredEditError::EmptyOldString);
    }
    if old_string == new_string {
        return Err(StructuredEditError::OldEqualsNew);
    }

    let count = content.matches(old_string).count();
    match (count, replace_all) {
        (0, _) => Err(StructuredEditError::ZeroMatches),
        (1, _) => Ok(content.replacen(old_string, new_string, 1)),
        (_, true) => Ok(content.replace(old_string, new_string)),
        (count, false) => Err(StructuredEditError::MultipleMatches { count }),
    }
}

/// Write already-verified file bytes through the executor filesystem.
///
/// This does not re-parse `action.patch`, so CRLF and missing final newlines
/// stay exactly as computed by [`apply_exact_replacement`].
pub async fn apply_verified_action(
    action: &ApplyPatchAction,
    options: ApplyPatchOptions,
    stdout: &mut impl std::io::Write,
    stderr: &mut impl std::io::Write,
    fs: &dyn ExecutorFileSystem,
    sandbox: Option<&FileSystemSandboxContext>,
) -> Result<AppliedPatchDelta, ApplyPatchFailure> {
    let mut delta = AppliedPatchDelta::empty();
    match apply_verified_updates(action, options, fs, sandbox, &mut delta).await {
        Ok(affected_paths) => {
            print_summary(&affected_paths, stdout).map_err(|error| {
                ApplyPatchFailure::new(ApplyPatchError::from(error), delta.clone())
            })?;
            Ok(delta)
        }
        Err(error) => {
            let msg = error.to_string();
            writeln!(stderr, "{msg}").map_err(|error| {
                ApplyPatchFailure::new(ApplyPatchError::from(error), delta.clone())
            })?;
            let error = if let Some(io) = error.downcast_ref::<std::io::Error>() {
                ApplyPatchError::from(io)
            } else {
                ApplyPatchError::IoError(IoError {
                    context: msg,
                    source: std::io::Error::other(error),
                })
            };
            Err(ApplyPatchFailure::new(error, delta))
        }
    }
}

async fn apply_verified_updates(
    action: &ApplyPatchAction,
    options: ApplyPatchOptions,
    fs: &dyn ExecutorFileSystem,
    sandbox: Option<&FileSystemSandboxContext>,
    delta: &mut AppliedPatchDelta,
) -> anyhow::Result<AffectedPaths> {
    if action.changes().is_empty() {
        anyhow::bail!("No files were modified.");
    }

    let mut modified = Vec::new();
    for (path, change) in action.changes() {
        let ApplyPatchFileChange::Update { new_content, .. } = change else {
            anyhow::bail!("verified write supports existing-file updates only");
        };
        let original_contents = match fs
            .read_file_text(
                path,
                ReadFileOptions {
                    follow_symlinks: options.follow_symlinks,
                },
                sandbox,
            )
            .await
        {
            Ok(content) => content,
            Err(error) => {
                delta.mark_inexact();
                return Err(error).with_context(|| {
                    format!("Failed to read file {}", path.inferred_native_path_string())
                });
            }
        };
        if let Err(error) = fs
            .write_file(
                path,
                new_content.clone().into_bytes(),
                WriteFileOptions {
                    follow_symlinks: options.follow_symlinks,
                },
                sandbox,
            )
            .await
        {
            delta.mark_inexact();
            return Err(error).with_context(|| {
                format!(
                    "Failed to write file {}",
                    path.inferred_native_path_string()
                )
            });
        }
        delta.push(AppliedPatchChange {
            path: path.clone(),
            change: AppliedPatchFileChange::Update {
                move_path: None,
                old_content: original_contents,
                overwritten_move_content: None,
                new_content: new_content.clone(),
            },
        });
        modified.push(
            path.basename()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(path.inferred_native_path_string())),
        );
    }

    Ok(AffectedPaths {
        added: Vec::new(),
        modified,
        deleted: Vec::new(),
    })
}

#[cfg(test)]
#[path = "structured_edit_tests.rs"]
mod tests;
