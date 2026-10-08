//! Validate indexed arrivals before either projection: transient search visibility
//! and the unchanged contiguous canonical stream. Never skip missing indexes.

use super::ResponsesStreamEvent;
use crate::common::ResponseEvent;
use crate::common::SearchActivityAdmission;
use crate::error::ApiError;
use codex_protocol::SearchActivityState;
use codex_protocol::grok_hosted::project_search_replay;
use codex_protocol::models::ResponseItem;
use eventsource_stream::Event;
use serde::Deserialize;
use std::collections::BTreeMap;

const MAX_PENDING_INDEXES: usize = 64;
const MAX_PENDING_EVENTS: usize = 4096;
const MAX_PENDING_BYTES: usize = 2 * 1024 * 1024;
const MAX_ITEM_ID_BYTES: usize = 1024;
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

#[derive(Deserialize)]
struct IndexedEvent {
    output_index: Option<u64>,
    #[serde(flatten)]
    event: ResponsesStreamEvent,
}

struct Lifetime {
    id: Option<String>,
    kind: String,
    closed: bool,
    web: bool,
    status_completed: bool,
}

impl Lifetime {
    fn bytes(&self) -> usize {
        self.kind.len() + self.id.as_ref().map_or(0, String::len)
    }
}

pub(super) struct GrokSequencer {
    next: u64,
    open: bool,
    pending: BTreeMap<u64, Vec<(Event, IndexedEvent)>>,
    // Includes the released-but-open canonical head. Evict on canonical done;
    // the watermark rejects late arrivals, without unbounded tombstones.
    lifetimes: BTreeMap<u64, Lifetime>,
    events: usize,
    bytes: usize,
    search_activity: SearchActivityAdmission,
}

impl GrokSequencer {
    pub(super) fn new(search_activity: SearchActivityAdmission) -> Self {
        Self {
            next: 0,
            open: false,
            pending: BTreeMap::new(),
            lifetimes: BTreeMap::new(),
            events: 0,
            bytes: 0,
            search_activity,
        }
    }

    fn admit(
        &mut self,
        index: u64,
        event: &ResponsesStreamEvent,
        frame_bytes: usize,
    ) -> Result<Option<ResponseEvent>, ApiError> {
        let mut state = None;
        match event.kind.as_str() {
            "response.output_item.added" => {
                if self.lifetimes.contains_key(&index) {
                    return Err(invalid("output item lifetime is out of order"));
                }
                let (item, id, kind) = parse_item(event)?;
                if id.as_ref().is_some_and(|id| {
                    self.lifetimes
                        .values()
                        .any(|bound| bound.id.as_ref() == Some(id))
                }) {
                    return Err(invalid("output item ID is already bound to another index"));
                }
                let web = self.search_activity == SearchActivityAdmission::Web
                    && matches!(item, ResponseItem::WebSearchCall { .. });
                if web
                    && !matches!(&item, ResponseItem::WebSearchCall {
                    id: Some(id), status: Some(status), ..
                } if !id.as_str().is_empty() && id.as_str().len() <= MAX_ITEM_ID_BYTES && matches!(status.as_str(), "in_progress" | "searching"))
                {
                    return Err(invalid("invalid Web search start"));
                }
                if web && index > MAX_SAFE_INTEGER {
                    return Err(invalid(
                        "search output index exceeds the public integer range",
                    ));
                }
                let bound = Lifetime {
                    id,
                    kind,
                    closed: false,
                    web,
                    status_completed: false,
                };
                if frame_bytes.saturating_add(bound.bytes())
                    > MAX_PENDING_BYTES.saturating_sub(self.bytes)
                {
                    return Err(invalid("pending output buffer limit exceeded"));
                }
                self.bytes += bound.bytes();
                self.lifetimes.insert(index, bound);
                if web {
                    state = Some(SearchActivityState::Running);
                }
            }
            _ => {
                let bound = self
                    .lifetimes
                    .get_mut(&index)
                    .ok_or_else(|| invalid("item event precedes output_item.added"))?;
                if bound.closed {
                    return Err(invalid("event follows a closed output item"));
                }
                if event
                    .item_id
                    .as_ref()
                    .is_some_and(|id| bound.id.as_ref().is_some_and(|bound_id| bound_id != id))
                {
                    return Err(invalid("item event identity differs from its output index"));
                }
                if event.kind == "response.output_item.done" {
                    let (item, id, kind) = parse_item(event)?;
                    // Stock non-Web completion can omit an ID assigned on start.
                    // Web observation authority always requires an exact nonempty ID.
                    let identity_changed = if bound.kind == "web_search_call" {
                        bound.id != id
                    } else {
                        bound
                            .id
                            .as_ref()
                            .zip(id.as_ref())
                            .is_some_and(|(a, b)| a != b)
                    };
                    if identity_changed || bound.kind != kind {
                        return Err(invalid(
                            "completed item differs from its admitted identity/type",
                        ));
                    }
                    if bound.web {
                        project_search_replay(&item)
                            .map_err(|_| invalid("invalid completed Web search"))?;
                        state = Some(SearchActivityState::Completed);
                    }
                    bound.closed = true;
                } else if event.kind.starts_with("response.web_search_call.") {
                    if bound.kind != "web_search_call"
                        || event.item_id != bound.id
                        || bound.id.is_none()
                    {
                        return Err(invalid("Web status has no matching admitted item"));
                    }
                    match event.kind.as_str() {
                        "response.web_search_call.completed" => bound.status_completed = true,
                        "response.web_search_call.in_progress"
                        | "response.web_search_call.searching"
                            if !bound.status_completed => {}
                        _ => return Err(invalid("invalid Web status transition")),
                    }
                }
            }
        }
        let Some(state) = state else {
            return Ok(None);
        };
        // Broken admission state must fail the stream, never panic or fabricate activity.
        let item_id = self
            .lifetimes
            .get(&index)
            .and_then(|bound| bound.id.as_ref())
            .filter(|id| !id.is_empty())
            .cloned()
            .ok_or_else(|| invalid("missing admitted Web item identity"))?;
        Ok(Some(ResponseEvent::SearchActivity {
            output_index: index,
            item_id,
            kind: codex_protocol::SearchActivityKind::Web,
            state,
        }))
    }

    pub(super) fn push(
        &mut self,
        raw: Event,
    ) -> Result<(Option<ResponseEvent>, Vec<Event>), ApiError> {
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
                || event.kind.starts_with("response.web_search_call.")
                || event.kind.ends_with(".delta")
            {
                return Err(invalid("item event is missing output_index"));
            }
            return Ok((None, vec![raw]));
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
        let activity = self.admit(index, event, bytes)?;
        self.pending.entry(index).or_default().push((raw, frame));
        self.events += 1;
        self.bytes += bytes;
        let mut ready = Vec::new();
        while let Some(queued) = self.pending.remove(&self.next) {
            for (raw, frame) in queued {
                self.events -= 1;
                self.bytes -= raw.data.len();
                match frame.event.kind.as_str() {
                    "response.output_item.added" => self.open = true,
                    "response.output_item.done" => {
                        self.open = false;
                        let bound = self
                            .lifetimes
                            .remove(&self.next)
                            .ok_or_else(|| invalid("missing admitted output item lifetime"))?;
                        self.bytes -= bound.bytes();
                        self.next = self
                            .next
                            .checked_add(1)
                            .ok_or_else(|| invalid("output index overflow"))?;
                    }
                    _ => {}
                }
                ready.push(raw);
            }
        }
        Ok((activity, ready))
    }
}

fn parse_item(
    event: &ResponsesStreamEvent,
) -> Result<(ResponseItem, Option<String>, String), ApiError> {
    let value = event
        .item
        .as_ref()
        .ok_or_else(|| invalid("missing output item"))?;
    let item: ResponseItem = serde_json::from_value(value.clone())
        .map_err(|_| invalid("malformed or unknown output item"))?;
    if matches!(item, ResponseItem::Other) {
        return Err(invalid("malformed or unknown output item"));
    }
    let id = value
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let kind = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid("missing output item type"))?
        .to_owned();
    Ok((item, id, kind))
}

fn invalid(reason: &str) -> ApiError {
    ApiError::Stream(format!("Grok stream: {reason}"))
}
