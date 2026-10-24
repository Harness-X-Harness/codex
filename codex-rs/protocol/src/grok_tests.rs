use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn x_search_options_validate_calendar_dates_and_merged_range() {
    for date in [
        "",
        "2026-1-01",
        "2026-01-1",
        "2026-01-01T00:00:00Z",
        " 2026-01-01",
        "2026-01-01 ",
        "２０２６-01-01",
        "2026/01/01",
        "+026-01-01",
        "2026-02-29",
        "1900-02-29",
        "2026-04-31",
        "2026-00-01",
        "2026-01-00",
    ] {
        for field in ["from_date", "to_date"] {
            let mut input = json!({});
            input[field] = json!(date);
            assert!(
                serde_json::from_value::<GrokXSearchOptions>(input).is_err(),
                "{field}: {date}"
            );
        }
    }
    for input in [
        json!({"from_date": 42}),
        json!({"from_date": "2026-10-09", "to_date": "2026-10-08"}),
        json!({"future_policy": null}),
    ] {
        assert!(serde_json::from_value::<GrokXSearchOptions>(input).is_err());
    }
    for date in ["2000-02-29", "2024-02-29", "2026-10-08"] {
        assert_eq!(
            serde_json::from_value::<GrokXSearchOptions>(
                json!({"from_date": date, "to_date": date})
            )
            .unwrap(),
            GrokXSearchOptions {
                from_date: Some(date.into()),
                to_date: Some(date.into())
            }
        );
    }
    for input in [json!({}), json!({"from_date": null, "to_date": null})] {
        assert_eq!(
            serde_json::from_value::<GrokXSearchOptions>(input).unwrap(),
            GrokXSearchOptions::default()
        );
    }
}
