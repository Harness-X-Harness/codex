use codex_core::context::ContextualUserFragment;
use codex_core::context::InternalContextSource;
use codex_core::context::InternalModelContextFragment;
use codex_core::context::without_update_plan_instructions;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::ThreadGoal;
use codex_utils_template::Template;
use std::sync::LazyLock;

static CONTINUATION_PROMPT_TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    parse_embedded_template(
        include_str!("../templates/goals/continuation.md"),
        "goals/continuation.md",
    )
});

static CONTINUATION_PROMPT_WITHOUT_UPDATE_PLAN: LazyLock<Template> = LazyLock::new(|| {
    parse_embedded_template(
        &without_update_plan_instructions(include_str!("../templates/goals/continuation.md")),
        "goals/continuation.md",
    )
});

static BUDGET_LIMIT_PROMPT_TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    parse_embedded_template(
        include_str!("../templates/goals/budget_limit.md"),
        "goals/budget_limit.md",
    )
});

static OBJECTIVE_UPDATED_PROMPT_TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    parse_embedded_template(
        include_str!("../templates/goals/objective_updated.md"),
        "goals/objective_updated.md",
    )
});

static HOST_EVALUATE_FOOTER_TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    parse_embedded_template(
        include_str!("../templates/goals/host_evaluate_footer.md"),
        "goals/host_evaluate_footer.md",
    )
});

/// Who owns completion language in the idle continuation prompt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GoalContinuationOwner<'a> {
    ModelCommit,
    HostEvaluate { next_step: Option<&'a str> },
}

fn parse_embedded_template(source: &str, template_name: &str) -> Template {
    match Template::parse(source) {
        Ok(template) => template,
        Err(err) => panic!("embedded template {template_name} is invalid: {err}"),
    }
}

pub(crate) fn budget_limit_steering_item(goal: &ThreadGoal) -> ResponseItem {
    goal_context_input_item(budget_limit_prompt(goal))
}

pub(crate) fn objective_updated_steering_item(goal: &ThreadGoal) -> ResponseItem {
    goal_context_input_item(objective_updated_prompt(goal))
}

pub(crate) fn continuation_steering_item(
    goal: &ThreadGoal,
    update_plan_enabled: bool,
    owner: GoalContinuationOwner<'_>,
) -> ResponseItem {
    goal_context_input_item(continuation_prompt(goal, update_plan_enabled, owner))
}

fn goal_context_input_item(prompt: String) -> ResponseItem {
    ContextualUserFragment::into(InternalModelContextFragment::new(
        InternalContextSource::from_static("goal"),
        prompt,
    ))
}

fn continuation_prompt(
    goal: &ThreadGoal,
    update_plan_enabled: bool,
    owner: GoalContinuationOwner<'_>,
) -> String {
    let objective = escape_xml_text(&goal.objective);
    let tokens_used = goal.tokens_used.to_string();
    let token_budget = goal
        .token_budget
        .map(|budget| budget.to_string())
        .unwrap_or_else(|| "none".to_string());
    let remaining_tokens = goal
        .token_budget
        .map(|budget| (budget - goal.tokens_used).max(0).to_string())
        .unwrap_or_else(|| "unbounded".to_string());

    let template = if update_plan_enabled {
        &*CONTINUATION_PROMPT_TEMPLATE
    } else {
        &*CONTINUATION_PROMPT_WITHOUT_UPDATE_PLAN
    };
    let mut prompt = template
        .render([
            ("objective", objective.as_str()),
            ("tokens_used", tokens_used.as_str()),
            ("token_budget", token_budget.as_str()),
            ("remaining_tokens", remaining_tokens.as_str()),
        ])
        .unwrap_or_else(|err| {
            panic!("embedded goals/continuation.md template failed to render: {err}")
        });
    match owner {
        GoalContinuationOwner::ModelCommit => prompt,
        GoalContinuationOwner::HostEvaluate { next_step } => {
            prompt = strip_model_commit_tool_instructions(&prompt);
            prompt.push_str(&host_evaluate_continuation_footer(next_step));
            prompt
        }
    }
}

fn strip_model_commit_tool_instructions(prompt: &str) -> String {
    const MARKERS: [&str; 2] = [
        "If the objective is achieved, call update_goal",
        "Blocked audit:",
    ];
    let cut = MARKERS
        .iter()
        .filter_map(|marker| prompt.find(marker))
        .min()
        .unwrap_or(prompt.len());
    prompt[..cut].trim_end().to_string()
}

fn host_evaluate_continuation_footer(next_step: Option<&str>) -> String {
    let next_step = next_step
        .map(str::trim)
        .filter(|step| !step.is_empty())
        .unwrap_or("Continue working toward the objective.");
    let footer = HOST_EVALUATE_FOOTER_TEMPLATE
        .render([("next_step", next_step)])
        .unwrap_or_else(|err| {
            panic!("embedded goals/host_evaluate_footer.md template failed to render: {err}")
        });
    format!("\n\n{footer}")
}

fn budget_limit_prompt(goal: &ThreadGoal) -> String {
    let objective = escape_xml_text(&goal.objective);
    let time_used_seconds = goal.time_used_seconds.to_string();
    let tokens_used = goal.tokens_used.to_string();
    let token_budget = goal
        .token_budget
        .map(|budget| budget.to_string())
        .unwrap_or_else(|| "none".to_string());

    BUDGET_LIMIT_PROMPT_TEMPLATE
        .render([
            ("objective", objective.as_str()),
            ("time_used_seconds", time_used_seconds.as_str()),
            ("tokens_used", tokens_used.as_str()),
            ("token_budget", token_budget.as_str()),
        ])
        .unwrap_or_else(|err| {
            panic!("embedded goals/budget_limit.md template failed to render: {err}")
        })
}

fn objective_updated_prompt(goal: &ThreadGoal) -> String {
    let objective = escape_xml_text(&goal.objective);
    let tokens_used = goal.tokens_used.to_string();
    let (token_budget, remaining_tokens) = match goal.token_budget {
        Some(token_budget) => (
            token_budget.to_string(),
            (token_budget - goal.tokens_used).max(0).to_string(),
        ),
        None => ("none".to_string(), "unknown".to_string()),
    };

    OBJECTIVE_UPDATED_PROMPT_TEMPLATE
        .render([
            ("objective", objective.as_str()),
            ("tokens_used", tokens_used.as_str()),
            ("token_budget", token_budget.as_str()),
            ("remaining_tokens", remaining_tokens.as_str()),
        ])
        .unwrap_or_else(|err| {
            panic!("embedded goals/objective_updated.md template failed to render: {err}")
        })
}

fn escape_xml_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
