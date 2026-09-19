//! Sequence Grok Responses output streams by `output_index`.
//!
//! Grok interleaves output items: a message can stay open while hosted
//! `web_search_call` / reasoning items run, then its text deltas resume.
//! Stock Codex consumes one `active_item` at a time and routes
//! `OutputTextDelta` (which drops `item_id`) to whatever is active.
//!
//! Invariant: between `response.output_item.added(i)` and
//! `response.output_item.done(i)` the consumer receives only index-`i` and
//! index-less events. Items are delivered in ascending `output_index`, which
//! is Grok's own `response.completed.output` order. Stock OpenAI never
//! constructs this type.

use crate::sse::ResponsesStreamEvent;
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// Holds later `output_index` frames until the current head item is done.
#[derive(Debug, Default)]
pub(crate) struct GrokOutputSequencer {
    head: Option<u64>,
    pending: BTreeMap<u64, Vec<ResponsesStreamEvent>>,
}

impl GrokOutputSequencer {
    /// Returns the events that are ready for the stock consumer.
    pub(crate) fn push(&mut self, event: ResponsesStreamEvent) -> Vec<ResponsesStreamEvent> {
        match event.output_index {
            None => {
                if matches!(
                    event.kind.as_str(),
                    "response.completed" | "response.incomplete" | "response.failed"
                ) {
                    let mut out = self.drain();
                    out.push(event);
                    out
                } else {
                    vec![event]
                }
            }
            Some(index) => match self.head {
                None => {
                    if event.kind == "response.output_item.added" {
                        self.head = Some(index);
                    }
                    vec![event]
                }
                Some(head) => match index.cmp(&head) {
                    Ordering::Equal => {
                        let done = event.kind == "response.output_item.done";
                        let mut out = vec![event];
                        if done {
                            self.head = None;
                            out.extend(self.flush_ready());
                        }
                        out
                    }
                    Ordering::Less => vec![event],
                    Ordering::Greater => {
                        self.pending.entry(index).or_default().push(event);
                        Vec::new()
                    }
                },
            },
        }
    }

    /// Releases every queued event in ascending index order, ignoring head.
    pub(crate) fn drain(&mut self) -> Vec<ResponsesStreamEvent> {
        self.head = None;
        std::mem::take(&mut self.pending)
            .into_values()
            .flatten()
            .collect()
    }

    fn flush_ready(&mut self) -> Vec<ResponsesStreamEvent> {
        let mut out = Vec::new();
        while self.head.is_none() {
            let Some((_, queued)) = self.pending.pop_first() else {
                break;
            };
            for event in queued {
                out.extend(self.push(event));
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "grok_stream_tests.rs"]
mod tests;
