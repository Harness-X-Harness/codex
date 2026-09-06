from openai_codex._workflow import ThreadWorkflow, ThreadWorkflowGetResponse, ThreadWorkflowStatus


def test_workflow_models_accept_camel_case_wire_payload() -> None:
    workflow = ThreadWorkflow.model_validate(
        {
            "threadId": "thread-1",
            "runId": "run-1",
            "name": "workflow",
            "status": "active",
            "pendingInstruction": "Compile the crate.",
            "result": None,
            "error": None,
            "createdAt": 1,
            "updatedAt": 2,
        }
    )
    assert workflow.thread_id == "thread-1"
    assert workflow.run_id == "run-1"
    assert workflow.status == ThreadWorkflowStatus.active
    assert workflow.pending_instruction == "Compile the crate."
    assert workflow.result is None
    assert workflow.error is None

    failed = ThreadWorkflow.model_validate(
        {
            "threadId": "thread-1",
            "runId": "run-2",
            "name": "workflow",
            "status": "failed",
            "pendingInstruction": None,
            "result": None,
            "error": "replay_diverged",
            "createdAt": 1,
            "updatedAt": 2,
        }
    )
    assert failed.status == ThreadWorkflowStatus.failed
    assert failed.error == "replay_diverged"

    get = ThreadWorkflowGetResponse.model_validate({"workflow": None})
    assert get.workflow is None
