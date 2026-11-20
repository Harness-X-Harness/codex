//! Explicit fixture authority for the Grok mock, independent of product projection.

use std::borrow::Cow;

use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub(super) struct GrokHostedReplay(Option<Vec<Value>>);

impl GrokHostedReplay {
    pub(super) fn new(completed_x_calls: &[Value]) -> Self {
        let mut replay = Vec::new();
        for item in completed_x_calls {
            let fields = item
                .as_object()
                .expect("hosted X fixture must be an object");
            assert_eq!(fields.len(), 6, "unsupported hosted X fixture fields");
            assert_eq!(item["type"], "custom_tool_call");
            assert_eq!(item["status"], "completed");
            assert!(matches!(
                item["name"].as_str(),
                Some("x_keyword_search" | "x_semantic_search" | "x_user_search" | "x_thread_fetch")
            ));
            for key in ["id", "call_id"] {
                assert!(
                    item[key].as_str().is_some_and(|value| !value.is_empty()),
                    "hosted X fixture requires a nonempty {key}"
                );
            }
            assert!(item["input"].is_string(), "hosted X input must be a string");
            let mut wire_item = item.clone();
            wire_item.as_object_mut().unwrap().remove("status");
            assert!(
                serde_json::to_vec(&wire_item).unwrap().len() <= 40_000,
                "hosted X fixture exceeds the replay byte limit"
            );
            assert!(!replay.contains(&wire_item), "duplicate hosted X fixture");
            replay.push(wire_item);
        }
        Self(Some(replay))
    }

    pub(super) fn local_items<'a>(&self, items: &'a [Value]) -> Cow<'a, [Value]> {
        let Some(fixtures) = &self.0 else {
            return Cow::Borrowed(items);
        };
        let mut expected = fixtures.iter().collect::<Vec<_>>();
        Cow::Owned(
            items
                .iter()
                .filter(|item| {
                    if let Some(index) = expected.iter().position(|fixture| *fixture == *item) {
                        expected.remove(index);
                        return false;
                    }
                    // Grok local calls are flattened to function_call. A raw
                    // custom call cannot buy fixture authority with an output.
                    assert_ne!(
                        item["type"], "custom_tool_call",
                        "unregistered or mutated Grok hosted X replay"
                    );
                    true
                })
                .cloned()
                .collect(),
        )
    }
}
