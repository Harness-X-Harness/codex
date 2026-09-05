from __future__ import annotations

from enum import Enum
from typing import Annotated

from pydantic import BaseModel, ConfigDict, Field


class ThreadWorkflowStatus(str, Enum):
    active = "active"
    paused = "paused"
    complete = "complete"


class ThreadWorkflowStep(BaseModel):
    model_config = ConfigDict(populate_by_name=True)
    id: str
    title: str
    instruction: str


class ThreadWorkflow(BaseModel):
    model_config = ConfigDict(populate_by_name=True)
    thread_id: Annotated[str, Field(alias="threadId")]
    run_id: Annotated[str, Field(alias="runId")]
    name: str
    status: ThreadWorkflowStatus
    current_step_index: Annotated[int, Field(alias="currentStepIndex")]
    steps: list[ThreadWorkflowStep]
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
