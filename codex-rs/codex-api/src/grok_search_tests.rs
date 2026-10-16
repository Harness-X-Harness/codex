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
