use crate::function_tool::FunctionCallError;
use codex_protocol::models::ResponseItem;
use codex_tools::GrokToolPlan;
use codex_tools::ToolSpec;
use codex_tools::is_evidence_backed_x_search_name;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GrokHostedOutputEventPhase {
    Added,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GrokHostedOutputOwner {
    Ordinary,
    WebSearch,
    XSearch,
    ImageGeneration,
    UnknownCustom,
}

impl GrokHostedOutputOwner {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Ordinary => "ordinary local output",
            Self::WebSearch => "Web Search",
            Self::XSearch => "X Search",
            Self::ImageGeneration => "Image Generation",
            Self::UnknownCustom => "unknown Custom",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GrokHostedOutput<'a> {
    Ordinary,
    Hosted {
        item_id: &'a str,
        owner: GrokHostedOutputOwner,
        projects_custom_output: bool,
    },
    UnknownCustom {
        item_id: Option<&'a str>,
        reason: String,
    },
}

pub(super) fn classify_grok_hosted_output<'a>(
    plan: Option<&GrokToolPlan>,
    item: &'a ResponseItem,
    phase: GrokHostedOutputEventPhase,
) -> Result<GrokHostedOutput<'a>, FunctionCallError> {
    let Some(plan) = plan else {
        return Ok(GrokHostedOutput::Ordinary);
    };

    match item {
        ResponseItem::WebSearchCall { id, status, .. } => {
            require_declaration(plan, "Web Search", |spec| {
                matches!(spec, ToolSpec::WebSearch { .. })
            })?;
            let item_id = validate_hosted_shape(
                id.as_deref(),
                status.as_deref(),
                /*has_call_id*/ true,
                phase,
                "Web Search",
            )?;
            Ok(GrokHostedOutput::Hosted {
                item_id,
                owner: GrokHostedOutputOwner::WebSearch,
                projects_custom_output: false,
            })
        }
        ResponseItem::GrokImageGenerationCall { id, status, .. } => {
            require_declaration(plan, "Image Generation", |spec| {
                matches!(spec, ToolSpec::ImageGeneration)
            })?;
            let item_id = validate_hosted_shape(
                id.as_deref(),
                Some(status.as_str()),
                /*has_call_id*/ true,
                phase,
                "Image Generation",
            )?;
            Ok(GrokHostedOutput::Hosted {
                item_id,
                owner: GrokHostedOutputOwner::ImageGeneration,
                projects_custom_output: false,
            })
        }
        ResponseItem::CustomToolCall {
            id,
            status,
            call_id,
            name,
            namespace,
            ..
        } => {
            let declared = plan
                .declarations
                .iter()
                .any(|spec| matches!(spec, ToolSpec::XSearch));
            let recognized =
                declared && namespace.is_none() && is_evidence_backed_x_search_name(name);
            let validation = recognized.then(|| {
                validate_hosted_shape(
                    id.as_deref(),
                    status.as_deref(),
                    !call_id.is_empty(),
                    phase,
                    "X Search",
                )
            });
            if let Some(Ok(item_id)) = validation {
                return Ok(GrokHostedOutput::Hosted {
                    item_id,
                    owner: GrokHostedOutputOwner::XSearch,
                    projects_custom_output: true,
                });
            }

            let reason = match validation {
                Some(Err(FunctionCallError::Fatal(reason))) => reason,
                Some(Err(other)) => other.to_string(),
                None if !declared => {
                    format!("Grok hosted custom output `{name}` was not declared for this turn")
                }
                None => format!("unknown Grok hosted custom output `{name}`"),
            };
            let item_id = id.as_deref().filter(|item_id| !item_id.is_empty());
            match phase {
                GrokHostedOutputEventPhase::Added => {
                    let item_id = validate_hosted_shape(
                        item_id,
                        status.as_deref(),
                        !call_id.is_empty(),
                        phase,
                        "unknown Custom",
                    )?;
                    Ok(GrokHostedOutput::UnknownCustom {
                        item_id: Some(item_id),
                        reason,
                    })
                }
                GrokHostedOutputEventPhase::Done => {
                    Ok(GrokHostedOutput::UnknownCustom { item_id, reason })
                }
            }
        }
        _ => Ok(GrokHostedOutput::Ordinary),
    }
}

fn require_declaration(
    plan: &GrokToolPlan,
    label: &str,
    owns: impl Fn(&ToolSpec) -> bool,
) -> Result<(), FunctionCallError> {
    if plan.declarations.iter().any(owns) {
        Ok(())
    } else {
        Err(FunctionCallError::Fatal(format!(
            "Grok {label} output was not declared for this turn"
        )))
    }
}

fn validate_hosted_shape<'a>(
    item_id: Option<&'a str>,
    status: Option<&str>,
    has_call_id: bool,
    phase: GrokHostedOutputEventPhase,
    label: &str,
) -> Result<&'a str, FunctionCallError> {
    let Some(item_id) = item_id.filter(|item_id| !item_id.is_empty()) else {
        return Err(FunctionCallError::Fatal(format!(
            "Grok {label} output is missing provider item_id"
        )));
    };
    if !has_call_id {
        return Err(FunctionCallError::Fatal(format!(
            "Grok {label} output is missing call_id"
        )));
    }
    let valid_status = match phase {
        GrokHostedOutputEventPhase::Added => status == Some("in_progress"),
        GrokHostedOutputEventPhase::Done => matches!(status, Some("completed" | "failed")),
    };
    if !valid_status {
        return Err(FunctionCallError::Fatal(format!(
            "Grok {label} {} has an invalid status",
            match phase {
                GrokHostedOutputEventPhase::Added => "start",
                GrokHostedOutputEventPhase::Done => "terminal",
            }
        )));
    }
    Ok(item_id)
}
