use super::*;

#[test]
fn x_search_policy_cannot_be_ignored_by_a_different_dialect() {
    for options in [
        GrokXSearchOptions::default(),
        GrokXSearchOptions {
            from_date: Some("2026-01-01".into()),
            to_date: None,
        },
    ] {
        let error = validate_grok_x_search(ApiDialect::OpenAi, Some(&options)).unwrap_err();
        assert!(matches!(error, ApiError::Stream(message)
            if message == "X Search options require the Grok Responses dialect"));
        assert!(validate_grok_x_search(ApiDialect::Grok, Some(&options)).is_ok());
    }
    for dialect in [ApiDialect::OpenAi, ApiDialect::Grok] {
        assert!(validate_grok_x_search(dialect, /*options*/ None).is_ok());
    }
}
