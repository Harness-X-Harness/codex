import pytest
from pydantic import ValidationError

from openai_codex.client import CodexClient
from openai_codex.generated.v2_all import ServerNotification, ThreadItem
from openai_codex.models import Notification, UnknownNotification


@pytest.mark.parametrize("tag", ["unknownProviderTool", "WebSearch", "xSearch"])
def test_thread_item_rejects_unknown_wire_tags(tag):
    with pytest.raises(ValidationError):
        ThreadItem.model_validate(
            {
                "type": tag,
                "id": "search-1",
                "query": "docs",
                "action": {"type": "search", "query": "docs", "queries": None},
                "results": None,
            }
        )


@pytest.mark.parametrize("method", ["item/providerProgress", "item/searchActivity"])
def test_unregistered_method_stays_an_unknown_notification(method):
    params = {"threadId": "thread-1", "turnId": "turn-1", "providerField": "value"}
    with pytest.raises(ValidationError):
        ServerNotification.model_validate({"method": method, "params": params})
    assert CodexClient()._coerce_notification(method, params) == Notification(
        method=method, payload=UnknownNotification(params=params)
    )


def test_web_search_item_notification_roundtrips_opaque_results():
    params = {
        "threadId": "thread-1",
        "turnId": "turn-1",
        "completedAtMs": 123,
        "item": {
            "type": "webSearch",
            "id": "search-1",
            "query": "docs",
            "action": {"type": "search", "query": "docs", "queries": None},
            "results": [{"type": "provider_result", "extra": {"source": "docs"}}],
        },
    }
    wire = {"method": "item/completed", "params": params, "emittedAtMs": 123}
    notification = ServerNotification.model_validate(wire)
    assert notification.model_dump(mode="json", by_alias=True) == wire
    assert (
        CodexClient()
        ._coerce_notification("item/completed", params)
        .payload.model_dump(mode="json", by_alias=True)
        == params
    )
