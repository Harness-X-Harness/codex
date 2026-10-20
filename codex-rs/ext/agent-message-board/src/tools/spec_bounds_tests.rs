use pretty_assertions::assert_eq;

#[test]
fn board_specs_keep_declared_count_and_offset_bounds() {
    for (name, field, minimum) in [
        ("get_channels", "limit", 1),
        ("read_post", "offset_chars", 0),
        ("read_post", "limit_chars", 1),
        ("read_thread", "max_chars_per_post", 1),
    ] {
        let spec = super::tool(
            name,
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
