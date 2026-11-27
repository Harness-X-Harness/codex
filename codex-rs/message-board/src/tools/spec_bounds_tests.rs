use super::MessageBoardTool;
use pretty_assertions::assert_eq;

#[test]
fn board_specs_keep_declared_count_and_offset_bounds() {
    for (tool, field, minimum) in [
        (MessageBoardTool::GetChannels, "limit", 1),
        (MessageBoardTool::ReadPost, "offset_chars", 0),
        (MessageBoardTool::ReadPost, "limit_chars", 1),
        (MessageBoardTool::ReadThread, "max_chars_per_post", 1),
    ] {
        let spec = super::tool(
            tool,
            Some("collaboration"),
            "Board",
            /*description_override*/ None,
        );
        let wire = serde_json::to_value(spec).expect("board spec");
        assert_eq!(
            wire["tools"][0]["parameters"]["properties"][field]["minimum"],
            minimum
        );
    }
}
