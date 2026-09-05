from openai_codex._workflow import ThreadWorkflow, ThreadWorkflowGetResponse, ThreadWorkflowStatus


def test_workflow_models_accept_camel_case_wire_payload() -> None:
    workflow = ThreadWorkflow.model_validate(
        {
            "threadId": "thread-1",
            "runId": "run-1",
            "name": "workflow",
            "status": "active",
            "currentStepIndex": 0,
            "steps": [
                {
                    "id": "ask",
                    "title": "ask",
                    "instruction": "Compile the crate.",
                }
            ],
            "createdAt": 1,
            "updatedAt": 2,
        }
    )
    assert workflow.thread_id == "thread-1"
    assert workflow.run_id == "run-1"
    assert workflow.status == ThreadWorkflowStatus.active
    assert workflow.current_step_index == 0
    assert workflow.steps[0].instruction == "Compile the crate."

    get = ThreadWorkflowGetResponse.model_validate({"workflow": None})
    assert get.workflow is None
