use std::sync::LazyLock;

use codex_core::context::ContextualUserFragment;
use codex_core::context::InternalContextSource;
use codex_core::context::InternalModelContextFragment;
use codex_protocol::models::ResponseItem;
use codex_utils_template::Template;

use crate::run::WorkflowRun;

static YIELD_TEMPLATE: LazyLock<Template> = LazyLock::new(|| {
    match Template::parse(include_str!("../templates/workflow/current_step.md")) {
        Ok(template) => template,
        Err(err) => panic!("embedded template workflow/current_step.md is invalid: {err}"),
    }
});

pub(crate) fn yield_steering_item(run: &WorkflowRun, instruction: &str) -> ResponseItem {
    let name = escape_xml_text(&run.name);
    let instruction = escape_xml_text(instruction);
    let prompt = YIELD_TEMPLATE
        .render([
            ("name", name.as_str()),
            ("instruction", instruction.as_str()),
        ])
        .unwrap_or_else(|err| {
            panic!("embedded workflow/current_step.md template failed to render: {err}")
        });
    ContextualUserFragment::into(InternalModelContextFragment::new(
        InternalContextSource::from_static("workflow"),
        prompt,
    ))
}

fn escape_xml_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
