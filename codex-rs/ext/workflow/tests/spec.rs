use pretty_assertions::assert_eq;

use codex_workflow_extension::MAX_WORKFLOW_STEPS;
use codex_workflow_extension::WorkflowParseError;
use codex_workflow_extension::WorkflowStep;
use codex_workflow_extension::parse_workflow_markdown;

#[test]
fn parses_named_markdown_steps() {
    let definition = parse_workflow_markdown(
        "# Review PR\n\n## Gather context\nRead the diff.\n\n## Write findings\nList bugs.\n",
    )
    .expect("parse");
    assert_eq!(definition.name, "Review PR");
    assert_eq!(
        definition.steps,
        vec![
            WorkflowStep {
                id: "gather_context".to_string(),
                title: "Gather context".to_string(),
                instruction: "Read the diff.".to_string(),
            },
            WorkflowStep {
                id: "write_findings".to_string(),
                title: "Write findings".to_string(),
                instruction: "List bugs.".to_string(),
            },
        ]
    );
}

#[test]
fn headingless_source_is_a_single_step() {
    let definition =
        parse_workflow_markdown("Ship the independent workflow engine.").expect("parse");
    assert_eq!(definition.name, "Work");
    assert_eq!(
        definition.steps,
        vec![WorkflowStep {
            id: "work".to_string(),
            title: "Work".to_string(),
            instruction: "Ship the independent workflow engine.".to_string(),
        }]
    );
}

#[test]
fn empty_source_is_rejected() {
    assert_eq!(
        parse_workflow_markdown("  \n"),
        Err(WorkflowParseError::Empty)
    );
}

#[test]
fn duplicate_titles_get_unique_ids() {
    let definition =
        parse_workflow_markdown("# Demo\n## Build\nDo A.\n## Build\nDo B.\n").expect("parse");
    assert_eq!(definition.steps[0].id, "build");
    assert_eq!(definition.steps[1].id, "build_2");
}

#[test]
fn too_many_steps_are_rejected() {
    let mut source = String::from("# Overflow\n");
    for index in 0..=MAX_WORKFLOW_STEPS {
        source.push_str(&format!("## Step {index}\nDo it.\n"));
    }
    assert_eq!(
        parse_workflow_markdown(&source),
        Err(WorkflowParseError::TooManySteps {
            actual: MAX_WORKFLOW_STEPS + 1
        })
    );
}
