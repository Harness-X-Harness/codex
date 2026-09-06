from __future__ import annotations

from enum import Enum
from typing import Annotated, Any

from pydantic import BaseModel, ConfigDict, Field


class ThreadWorkflowStatus(str, Enum):
    active = "active"
    paused = "paused"
    complete = "complete"
    waiting = "waiting"


class ThreadWorkflow(BaseModel):
    model_config = ConfigDict(populate_by_name=True)
    thread_id: Annotated[str, Field(alias="threadId")]
    run_id: Annotated[str, Field(alias="runId")]
    name: str
    status: ThreadWorkflowStatus
    pending_instruction: Annotated[str | None, Field(alias="pendingInstruction")]
    result: Any
    created_at: Annotated[int, Field(alias="createdAt")]
    updated_at: Annotated[int, Field(alias="updatedAt")]


class ThreadWorkflowGetResponse(BaseModel):
    model_config = ConfigDict(populate_by_name=True)
    workflow: ThreadWorkflow | None


class ThreadWorkflowStartResponse(BaseModel):
    model_config = ConfigDict(populate_by_name=True)
    workflow: ThreadWorkflow


class ThreadWorkflowAdvanceResponse(BaseModel):
    model_config = ConfigDict(populate_by_name=True)
    workflow: ThreadWorkflow


class ThreadWorkflowStopResponse(BaseModel):
    model_config = ConfigDict(populate_by_name=True)
    workflow: ThreadWorkflow


class ThreadWorkflowResumeResponse(BaseModel):
    model_config = ConfigDict(populate_by_name=True)
    workflow: ThreadWorkflow
