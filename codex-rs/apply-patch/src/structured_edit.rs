//! Exact UTF-8 edits, with the planned snapshot checked by the executor's CAS.

use std::path::PathBuf;

use anyhow::Context;
use codex_exec_server::ConditionalWriteResult;
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

/// Why an exact replacement was rejected before any write.
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

/// The target no longer matches the snapshot used to plan and approve the edit.
pub const STALE_STRUCTURED_EDIT_MESSAGE: &str =
    "file changed after the edit was planned; re-read the file and submit a new structured_edit";

/// Replaces exact, nonoverlapping UTF-8 substrings without normalizing any bytes.
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
    match (content.matches(old_string).count(), replace_all) {
        (0, _) => Err(StructuredEditError::ZeroMatches),
        (1, _) => Ok(content.replacen(old_string, new_string, 1)),
        (_, true) => Ok(content.replace(old_string, new_string)),
        (count, false) => Err(StructuredEditError::MultipleMatches { count }),
    }
}

/// Commits one exact update through the selected executor and sandbox policy.
///
/// Only a non-moving update with a planned snapshot is supported. The synthetic
/// patch is never parsed. Conflict and uncertain errors are returned without an
/// ordinary-write fallback, recomputation, or another conditional-write attempt.
pub async fn apply_verified_action(
    action: &ApplyPatchAction,
    options: ApplyPatchOptions,
    stdout: &mut impl std::io::Write,
    stderr: &mut impl std::io::Write,
    fs: &dyn ExecutorFileSystem,
    sandbox: Option<&FileSystemSandboxContext>,
) -> Result<AppliedPatchDelta, ApplyPatchFailure> {
    let mut delta = AppliedPatchDelta::empty();
    let mut retryable_read_denial = false;
    match apply_verified_update(
        action,
        options,
        fs,
        sandbox,
        &mut delta,
        &mut retryable_read_denial,
    )
    .await
    {
        Ok(affected_paths) => {
            print_summary(&affected_paths, stdout).map_err(|error| {
                ApplyPatchFailure::new(ApplyPatchError::from(error), delta.clone())
            })?;
            Ok(delta)
        }
        Err(error) => {
            // Preserve the actual cause for diagnostics; retry uses provenance below.
            let msg = format!("{error:#}");
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
            Err(ApplyPatchFailure {
                error,
                delta,
                retryable_read_denial,
            })
        }
    }
}

async fn apply_verified_update(
    action: &ApplyPatchAction,
    options: ApplyPatchOptions,
    fs: &dyn ExecutorFileSystem,
    sandbox: Option<&FileSystemSandboxContext>,
    delta: &mut AppliedPatchDelta,
    retryable_read_denial: &mut bool,
) -> anyhow::Result<AffectedPaths> {
    anyhow::ensure!(
        action.changes().len() == 1,
        "verified write requires exactly one existing-file update"
    );
    let Some((
        path,
        ApplyPatchFileChange::Update {
            new_content,
            expected_content: Some(expected_content),
            move_path: None,
            ..
        },
    )) = action.changes().iter().next()
    else {
        anyhow::bail!("verified write requires a non-moving update with an original snapshot");
    };
    let original_content = match fs
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
            // A native read denial is known to precede mutation. Do not infer
            // this from error text: paths and remote diagnostics are untrusted.
            *retryable_read_denial = error.kind() == std::io::ErrorKind::PermissionDenied;
            delta.exact = false;
            return Err(error).with_context(|| {
                format!("Failed to read file {}", path.inferred_native_path_string())
            });
        }
    };
    anyhow::ensure!(
        original_content == *expected_content,
        "{STALE_STRUCTURED_EDIT_MESSAGE}"
    );
    match fs
        .write_file_if_unchanged(
            path,
            expected_content.as_bytes().to_vec(),
            new_content.as_bytes().to_vec(),
            WriteFileOptions {
                follow_symlinks: options.follow_symlinks,
            },
            sandbox,
        )
        .await
    {
        Ok(ConditionalWriteResult::Written) => {}
        Ok(ConditionalWriteResult::Conflict) => anyhow::bail!("{STALE_STRUCTURED_EDIT_MESSAGE}"),
        Err(error) => {
            // A failed or disconnected write may already have affected the file.
            delta.exact = false;
            return Err(error).with_context(|| {
                format!(
                    "Failed to write file {}",
                    path.inferred_native_path_string()
                )
            });
        }
    }
    delta.changes.push(AppliedPatchChange {
        path: path.clone(),
        change: AppliedPatchFileChange::Update {
            move_path: None,
            old_content: original_content,
            overwritten_move_content: None,
            new_content: new_content.clone(),
        },
    });
    Ok(AffectedPaths {
        added: Vec::new(),
        modified: vec![
            path.basename()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(path.inferred_native_path_string())),
        ],
        deleted: Vec::new(),
    })
}

#[cfg(test)]
#[path = "structured_edit_tests.rs"]
mod tests;
