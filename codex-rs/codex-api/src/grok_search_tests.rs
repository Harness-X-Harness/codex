use super::*;
use pretty_assertions::assert_eq;

#[test]
fn web_domain_restrictions_are_preserved_and_empty_filters_are_omitted() {
    for filters in [
        json!(null),
        json!({}),
        json!({"allowed_domains": [], "excluded_domains": []}),
        json!({"allowed_domains": ["a.com", "b.com", "c.com", "d.com", "e.com"]}),
        json!({"allowed_domains": [], "excluded_domains": ["a.com"]}),
    ] {
        let input = json!({"type": "web_search", "filters": filters});
        let mut expected = json!({"type": "web_search"});
        if let Some(allowed) = filters
            .get("allowed_domains")
            .filter(|value| !value.as_array().unwrap().is_empty())
        {
            expected["filters"] = json!({"allowed_domains": allowed});
        }
        if let Some(excluded) = filters
            .get("excluded_domains")
            .filter(|value| !value.as_array().unwrap().is_empty())
        {
            expected["filters"] = json!({"excluded_domains": excluded});
        }
        assert_eq!(
            project_web_search(input.as_object().unwrap(), /*index*/ 0).unwrap(),
            expected
        );
    }
}

#[test]
fn web_rejects_malformed_and_unsupported_restrictions() {
    for restriction in [
        json!({"filters": []}),
        json!({"filters": "invalid"}),
        json!({"filters": {"allowed_domains": null}}),
        json!({"filters": {"allowed_domains": "a.com"}}),
        json!({"filters": {"allowed_domains": [""]}}),
        json!({"filters": {"allowed_domains": [" \t"]}}),
        json!({"filters": {"allowed_domains": [42]}}),
        json!({"filters": {"excluded_domains": ["a", "b", "c", "d", "e", "f"]}}),
        json!({"filters": {"allowed_domains": ["a.com"], "excluded_domains": ["b.com"]}}),
        json!({"filters": {"blocked_domains": ["a.com"]}}),
        json!({"filters": {"future_policy": null}}),
        json!({"allowed_domains": ["a.com"]}),
        json!({"user_location": {"type": "approximate", "country": "US"}}),
        json!({"search_context_size": "high"}),
        json!({"search_content_types": []}),
        json!({"external_web_access": "false"}),
        json!({"indexed_web_access": 0}),
        json!({"enable_image_understanding": true}),
    ] {
        assert!(
            project_web_search(restriction.as_object().unwrap(), /*index*/ 0).is_err(),
            "{restriction}"
        );
    }
}

#[test]
fn web_mode_hints_are_validated_and_omitted() {
    for mode in [json!(null), json!(true), json!(false)] {
        let input = json!({"type": "web_search", "external_web_access": mode,
            "indexed_web_access": mode, "search_context_size": null,
            "search_content_types": null, "user_location": null});
        assert_eq!(
            project_web_search(input.as_object().unwrap(), /*index*/ 0).unwrap(),
            json!({"type": "web_search"})
        );
    }
}

#[test]
fn x_dates_override_defaults_independently_and_null_falls_back() {
    let defaults = GrokXSearchOptions {
        from_date: Some("2024-02-01".into()),
        to_date: Some("2024-03-31".into()),
    };
    for (input, expected) in [
        (
            json!({"type": "x_search", "from_date": "2024-02-29"}),
            json!({"type": "x_search", "from_date": "2024-02-29", "to_date": "2024-03-31"}),
        ),
        (
            json!({"type": "x_search", "from_date": null, "to_date": "2024-03-01"}),
            json!({"type": "x_search", "from_date": "2024-02-01", "to_date": "2024-03-01"}),
        ),
        (
            json!({"type": "x_search", "from_date": "2024-02-29", "to_date": "2024-02-29"}),
            json!({"type": "x_search", "from_date": "2024-02-29", "to_date": "2024-02-29"}),
        ),
    ] {
        assert_eq!(
            project_x_search(input.as_object().unwrap(), Some(&defaults)).unwrap(),
            expected
        );
    }
    assert_eq!(
        project_x_search(
            json!({"type": "x_search", "from_date": null})
                .as_object()
                .unwrap(),
            /*defaults*/ None
        )
        .unwrap(),
        json!({"type": "x_search"})
    );
}

#[test]
fn x_rejects_unknown_fields_bad_values_and_reversed_merged_ranges() {
    let defaults = GrokXSearchOptions {
        from_date: Some("2026-01-01".into()),
        to_date: None,
    };
    for input in [
        json!({"type": "x_search", "from_date": 20260101}),
        json!({"type": "x_search", "from_date": ""}),
        json!({"type": "x_search", "from_date": "2026-02-29"}),
        json!({"type": "x_search", "to_date": "2025-12-31"}),
        json!({"type": "x_search", "allowed_x_handles": ["xai"]}),
        json!({"type": "x_search", "filters": null}),
        json!({"type": "x_search", "future_policy": false}),
    ] {
        assert!(
            project_x_search(input.as_object().unwrap(), Some(&defaults)).is_err(),
            "{input}"
        );
    }
}
