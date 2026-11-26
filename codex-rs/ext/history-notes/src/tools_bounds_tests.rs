use pretty_assertions::assert_eq;
use serde_json::Value;

use super::tool_spec;
use crate::requests::HistoryListWindows;
use crate::requests::HistoryReadItem;
use crate::requests::NotesSearchContents;

#[test]
fn history_tool_specs_keep_declared_count_and_offset_bounds() {
    for (spec, field, minimum) in [
        (
            tool_spec::<HistoryListWindows, Value>(None, None),
            "limit",
            1.0,
        ),
        (
            tool_spec::<HistoryReadItem, Value>(None, None),
            "offset_chars",
            0.0,
        ),
        (
            tool_spec::<NotesSearchContents, Value>(None, None),
            "max_matches_per_file",
            1.0,
        ),
    ] {
        let wire = serde_json::to_value(spec).expect("history spec");
        let property = &wire["tools"][0]["parameters"]["properties"][field];
        assert_eq!(property["type"], "integer");
        assert_eq!(property["minimum"].as_f64(), Some(minimum));
    }
}
