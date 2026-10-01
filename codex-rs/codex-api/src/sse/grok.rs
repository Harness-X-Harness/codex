//! Normalize indexed item lifetimes into a contiguous prefix for stock ingress.
//! Keep per-item arrival order; never drain open items or skip missing indexes.

use super::ResponsesStreamEvent;
use crate::error::ApiError;
use codex_protocol::models::ResponseItem;
use eventsource_stream::Event;
use serde::Deserialize;
use std::collections::BTreeMap;

const MAX_PENDING_INDEXES: usize = 64;
const MAX_PENDING_EVENTS: usize = 4096;
const MAX_PENDING_BYTES: usize = 2 * 1024 * 1024;

#[derive(Deserialize)]
struct IndexedEvent {
    output_index: Option<u64>,
    #[serde(flatten)]
    event: ResponsesStreamEvent,
}

#[derive(Default)]
pub(super) struct GrokSequencer {
    next: u64,
    open: bool,
    pending: BTreeMap<u64, Vec<(Event, IndexedEvent)>>,
    events: usize,
    bytes: usize,
}

impl GrokSequencer {
    pub(super) fn push(&mut self, raw: Event) -> Result<Vec<Event>, ApiError> {
        let frame: IndexedEvent =
            serde_json::from_str(&raw.data).map_err(|_| invalid("malformed SSE event"))?;
        let event = &frame.event;
        let bytes = raw.data.len();
        let Some(index) = frame.output_index else {
            if event.kind == "response.completed" {
                let count = event
                    .response
                    .as_ref()
                    .and_then(|response| response.get("output"));
                if self.open
                    || !self.pending.is_empty()
                    || count.is_some_and(|output| {
                        output.as_array().map(Vec::len) != usize::try_from(self.next).ok()
                    })
                {
                    return Err(invalid(
                        "completion has an open item or missing output index",
                    ));
                }
            } else if event.kind.starts_with("response.output_")
                || event.kind.starts_with("response.reasoning_")
                || event.kind.starts_with("response.content_part.")
                || event.kind.starts_with("response.function_call_arguments.")
                || event.kind.starts_with("response.custom_tool_call_input.")
                || event.kind.ends_with(".delta")
            {
                return Err(invalid("item event is missing output_index"));
            }
            return Ok(vec![raw]);
        };
        if matches!(
            event.kind.as_str(),
            "response.completed" | "response.failed" | "response.incomplete" | "error"
        ) {
            return Err(invalid("terminal event carries output_index"));
        }
        if index < self.next {
            return Err(invalid("late output index"));
        }
        if (!self.pending.contains_key(&index) && self.pending.len() == MAX_PENDING_INDEXES)
            || self.events == MAX_PENDING_EVENTS
            || bytes > MAX_PENDING_BYTES.saturating_sub(self.bytes)
        {
            return Err(invalid("pending output buffer limit exceeded"));
        }
        self.pending.entry(index).or_default().push((raw, frame));
        self.events += 1;
        self.bytes += bytes;
        let mut ready = Vec::new();
        while let Some(queued) = self.pending.remove(&self.next) {
            for (raw, frame) in queued {
                self.events -= 1;
                self.bytes -= raw.data.len();
                let event = frame.event;
                if frame.output_index != Some(self.next) {
                    return Err(invalid("event follows a closed output item"));
                }
                match event.kind.as_str() {
                    "response.output_item.added" | "response.output_item.done" => {
                        let added = event.kind == "response.output_item.added";
                        if self.open == added {
                            return Err(invalid("output item lifetime is out of order"));
                        }
                        let item = event.item.ok_or_else(|| invalid("missing output item"))?;
                        if matches!(
                            serde_json::from_value::<ResponseItem>(item),
                            Err(_) | Ok(ResponseItem::Other)
                        ) {
                            return Err(invalid("malformed or unknown output item"));
                        }
                        self.open = added;
                        if !added {
                            self.next = self
                                .next
                                .checked_add(1)
                                .ok_or_else(|| invalid("output index overflow"))?;
                        }
                    }
                    _ if !self.open => {
                        return Err(invalid("item event precedes output_item.added"));
                    }
                    _ => {}
                }
                ready.push(raw);
            }
        }
        Ok(ready)
    }
}

fn invalid(reason: &str) -> ApiError {
    ApiError::Stream(format!("Grok stream: {reason}"))
}
