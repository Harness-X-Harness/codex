use super::*;

#[test]
fn retired_x_search_policy_is_rejected_for_every_dialect() {
    for options in [
        GrokXSearchOptions::default(),
        GrokXSearchOptions {
            from_date: Some("2026-01-01".into()),
            to_date: None,
        },
    ] {
        let error = validate_grok_x_search(ApiDialect::OpenAi, Some(&options)).unwrap_err();
        assert!(matches!(error, ApiError::Stream(message)
            if message.contains("Hosted X Search is retired")));
        assert!(validate_grok_x_search(ApiDialect::Grok, Some(&options)).is_err());
    }
    for dialect in [ApiDialect::OpenAi, ApiDialect::Grok] {
        assert!(validate_grok_x_search(dialect, /*options*/ None).is_ok());
    }
}
