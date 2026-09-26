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
//!
//! A late frame whose `output_index` is lower than the open head violates that
//! invariant and is rejected. Pending later indexes are bounded so a malformed
//! stream cannot buffer without limit. Terminal completion drains pending
//! items in ascending index order, then emits the terminal event.

use crate::sse::ResponsesStreamEvent;
use std::cmp::Ordering;
use std::collections::BTreeMap;

const MAX_PENDING_INDEXES: usize = 64;
const MAX_PENDING_EVENTS: usize = 4096;

/// Why a Grok SSE frame cannot be sequenced.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum GrokStreamError {
    #[error(
        "Grok stream violated output_index sequencing: late index {index} while head {head} is open"
    )]
    LateLowerIndex { index: u64, head: u64 },
    #[error("Grok stream pending output_index set exceeded {limit}")]
    PendingIndexLimit { limit: usize },
    #[error("Grok stream pending event buffer exceeded {limit}")]
    PendingEventLimit { limit: usize },
}

/// Holds later `output_index` frames until the current head item is done.
#[derive(Debug, Default)]
pub(crate) struct GrokOutputSequencer {
    head: Option<u64>,
    pending: BTreeMap<u64, Vec<ResponsesStreamEvent>>,
    pending_events: usize,
}

impl GrokOutputSequencer {
    /// Returns the events that are ready for the stock consumer.
    pub(crate) fn push(
        &mut self,
        event: ResponsesStreamEvent,
    ) -> Result<Vec<ResponsesStreamEvent>, GrokStreamError> {
        match event.output_index {
            None => {
                if matches!(
                    event.kind.as_str(),
                    "response.completed" | "response.incomplete" | "response.failed"
                ) {
                    let mut out = self.drain();
                    out.push(event);
                    Ok(out)
                } else {
                    Ok(vec![event])
                }
            }
            Some(index) => match self.head {
                None => {
                    if event.kind == "response.output_item.added" {
                        self.head = Some(index);
                    }
                    Ok(vec![event])
                }
                Some(head) => match index.cmp(&head) {
                    Ordering::Equal => {
                        let done = event.kind == "response.output_item.done";
                        let mut out = vec![event];
                        if done {
                            self.head = None;
                            out.extend(self.flush_ready()?);
                        }
                        Ok(out)
                    }
                    Ordering::Less => Err(GrokStreamError::LateLowerIndex { index, head }),
                    Ordering::Greater => {
                        self.buffer(index, event)?;
                        Ok(Vec::new())
                    }
                },
            },
        }
    }

    /// Releases every queued event in ascending index order, ignoring head.
    pub(crate) fn drain(&mut self) -> Vec<ResponsesStreamEvent> {
        self.head = None;
        self.pending_events = 0;
        std::mem::take(&mut self.pending)
            .into_values()
            .flatten()
            .collect()
    }

    fn buffer(&mut self, index: u64, event: ResponsesStreamEvent) -> Result<(), GrokStreamError> {
        if !self.pending.contains_key(&index) && self.pending.len() >= MAX_PENDING_INDEXES {
            return Err(GrokStreamError::PendingIndexLimit {
                limit: MAX_PENDING_INDEXES,
            });
        }
        if self.pending_events >= MAX_PENDING_EVENTS {
            return Err(GrokStreamError::PendingEventLimit {
                limit: MAX_PENDING_EVENTS,
            });
        }
        self.pending.entry(index).or_default().push(event);
        self.pending_events += 1;
        Ok(())
    }

    fn flush_ready(&mut self) -> Result<Vec<ResponsesStreamEvent>, GrokStreamError> {
        let mut out = Vec::new();
        while self.head.is_none() {
            let Some((_, queued)) = self.pending.pop_first() else {
                break;
            };
            self.pending_events = self.pending_events.saturating_sub(queued.len());
            for event in queued {
                out.extend(self.push(event)?);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
#[path = "grok_stream_tests.rs"]
mod tests;
