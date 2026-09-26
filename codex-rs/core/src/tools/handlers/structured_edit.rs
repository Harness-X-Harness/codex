use crate::function_tool::FunctionCallError;
use crate::session::turn_context::TurnEnvironment;
use crate::tools::context::ApplyPatchToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::apply_patch::execute_verified_patch;
use crate::tools::handlers::parse_arguments;
use crate::tools::handlers::resolve_tool_environment;
use crate::tools::handlers::structured_edit_spec::STRUCTURED_EDIT_TOOL_NAME;
use crate::tools::handlers::structured_edit_spec::create_structured_edit_tool;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use crate::tools::runtimes::apply_patch::ApplyPatchWriteMode;
use crate::tools::sandboxing::ToolCtx;
use codex_apply_patch::ApplyPatchAction;
use codex_apply_patch::apply_exact_replacement;
use codex_exec_server::GetMetadataOptions;
use codex_exec_server::ReadFileOptions;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;
use std::sync::Arc;

/// Function-tool editor that compiles an exact replacement into the existing
/// Codex file-change execution path.
#[derive(Default)]
pub struct StructuredEditHandler {
    multi_environment: bool,
}

impl StructuredEditHandler {
    pub(crate) fn new(multi_environment: bool) -> Self {
        Self { multi_environment }
    }
}

#[derive(Deserialize)]
struct StructuredEditArgs {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
    #[serde(default)]
    environment_id: Option<String>,
}

impl ToolExecutor<ToolInvocation> for StructuredEditHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(STRUCTURED_EDIT_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        create_structured_edit_tool(self.multi_environment)
    }

    fn handle<'a>(&'a self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(self.handle_call(invocation))
    }
}

impl CoreToolRuntime for StructuredEditHandler {}

impl StructuredEditHandler {
    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let ToolInvocation {
            session,
            step_context,
            cancellation_token,
            tracker,
            call_id,
            tool_name,
            payload,
            ..
        } = invocation;

        let ToolPayload::Function { arguments } = payload else {
            return Err(FunctionCallError::RespondToModel(
                "structured_edit handler received unsupported payload".to_string(),
            ));
        };
        let StructuredEditArgs {
            file_path,
            old_string,
            new_string,
            replace_all,
            environment_id,
        } = parse_arguments(&arguments)?;
        if environment_id.is_some() && !self.multi_environment {
            return Err(FunctionCallError::RespondToModel(
                "structured_edit environment selection is unavailable for this turn".to_string(),
            ));
        }

        let Some(turn_environment) =
            resolve_tool_environment(&step_context.environments, environment_id.as_deref())?
        else {
            return Err(FunctionCallError::RespondToModel(
                "structured_edit is unavailable in this session".to_string(),
            ));
        };
        let action = prepare_structured_edit_action(
            turn_environment,
            &file_path,
            &old_string,
            &new_string,
            replace_all,
        )
        .await?;
        let tool_ctx = ToolCtx {
            session,
            step_context: Arc::clone(&step_context),
            cancellation_token,
            call_id,
            tool_name,
        };
        let content = execute_verified_patch(
            action,
            turn_environment.clone(),
            Some(&tracker),
            tool_ctx,
            ApplyPatchWriteMode::VerifiedContents,
        )
        .await?;
        Ok(boxed_tool_output(ApplyPatchToolOutput::from_text(content)))
    }
}

async fn prepare_structured_edit_action(
    turn_environment: &TurnEnvironment,
    file_path: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<ApplyPatchAction, FunctionCallError> {
    let cwd = turn_environment.cwd().clone();
    let path_uri = cwd.join(file_path).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "unable to resolve file_path `{file_path}` against environment cwd `{cwd}`: {err}"
        ))
    })?;
    let model_visible_path = path_uri.inferred_native_path_string();
    let fs = turn_environment.environment.get_filesystem();
    let sandbox = turn_environment.sandbox_context(/*additional_permissions*/ None);
    let metadata = fs
        .get_metadata(&path_uri, GetMetadataOptions::default(), Some(&sandbox))
        .await
        .map_err(|error| {
            FunctionCallError::RespondToModel(format!(
                "structured_edit can only edit existing text files: unable to locate `{model_visible_path}`: {error}"
            ))
        })?;
    if !metadata.is_file {
        return Err(FunctionCallError::RespondToModel(format!(
            "structured_edit can only edit existing text files: `{model_visible_path}` is not a file"
        )));
    }
    let content = fs
        .read_file_text(&path_uri, ReadFileOptions::default(), Some(&sandbox))
        .await
        .map_err(|error| {
            FunctionCallError::RespondToModel(format!(
                "structured_edit failed to read `{model_visible_path}`: {error}"
            ))
        })?;
    let new_content = apply_exact_replacement(&content, old_string, new_string, replace_all)
        .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
    Ok(ApplyPatchAction::from_exact_update(
        cwd,
        path_uri,
        &content,
        new_content,
    ))
}

#[cfg(test)]
#[path = "structured_edit_tests.rs"]
mod tests;
