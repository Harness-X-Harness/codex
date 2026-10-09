use super::*;
use pretty_assertions::assert_eq;

#[test]
fn search_activity_labels_are_transient_and_completion_stops_animation() {
    let mut cell = SearchActivityCell::new(
        "web".into(),
        codex_app_server_protocol::SearchActivityKind::Web,
        /*animations_enabled*/ true,
    );
    assert!(cell.transcript_animation_tick().is_some());
    insta::assert_snapshot!(cell.raw_lines()[0].to_string(), @"Searching the web");
    cell.complete();
    assert_eq!(cell.transcript_animation_tick(), None);
    insta::assert_snapshot!(cell.display_lines(/*width*/ 80)[0].to_string(), @"• Web search completed");
    insta::assert_snapshot!(cell.transcript_lines(/*width*/ 18).iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"), @"
    • Web search
      completed
    ");
}

#[test]
fn web_action_labels_and_missing_details() {
    let url = "https://example.com/docs";
    let actions = [
        WebSearchAction::OpenPage {
            url: Some(url.into()),
        },
        WebSearchAction::OpenPage { url: None },
        WebSearchAction::FindInPage {
            url: Some(url.into()),
            pattern: Some("needle".into()),
        },
        WebSearchAction::FindInPage {
            url: None,
            pattern: Some("needle".into()),
        },
        WebSearchAction::FindInPage {
            url: Some(url.into()),
            pattern: Some(String::new()),
        },
        WebSearchAction::FindInPage {
            url: None,
            pattern: None,
        },
    ];
    let rendered = actions
        .into_iter()
        .flat_map(|action| {
            new_web_search_call("call".into(), String::new(), action)
                .display_lines(/*width*/ 80)
        })
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(rendered);
}

#[test]
fn web_action_compact_display_preserves_transcript_and_raw_details() {
    let url = "https://example.com/docs/very-long-page?section=narrowing#details";
    let cell = new_web_search_call(
        "call".into(),
        String::new(),
        WebSearchAction::FindInPage {
            url: Some(url.into()),
            pattern: Some("日本語 🦀 needle".into()),
        },
    );
    let display = cell.display_lines(/*width*/ 32);
    assert_eq!(display.len(), 1);
    assert!(display[0].width() <= 32);
    insta::assert_snapshot!(display[0].to_string());
    let full = format!("Searched for '日本語 🦀 needle' in {url}");
    assert_eq!(cell.raw_lines(), vec![Line::from(full.clone())]);
    assert_eq!(
        cell.transcript_lines(/*width*/ 200)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec![format!("• {full}")]
    );
}

#[test]
fn pending_web_action_and_legacy_query() {
    let cell = new_active_web_search_call(
        "call".into(),
        String::new(),
        /*animations_enabled*/ false,
    );
    insta::assert_snapshot!(cell.display_lines(/*width*/ 80)[0].to_string(), @"• Browsing the web");
    let legacy = new_web_search_call("call".into(), "old query".into(), WebSearchAction::Other);
    assert_eq!(
        legacy.raw_lines(),
        vec![Line::from("Searched the web for old query")]
    );
}

#[test]
fn batched_search_retains_each_query() {
    let cell = new_web_search_call(
        "call".into(),
        String::new(),
        WebSearchAction::Search {
            query: None,
            queries: Some(vec!["first query".into(), "second query".into()]),
        },
    );
    assert_eq!(
        cell.raw_lines(),
        vec![Line::from("Searched the web for first query, second query")]
    );
    insta::assert_snapshot!(cell.display_lines(/*width*/ 80)[0].to_string(), @"• Searched the web for first query, second query");
}

#[test]
fn search_activity_x_completion_stops_animation_and_wraps_truthfully() {
    let mut cell = SearchActivityCell::new(
        "x".into(),
        codex_app_server_protocol::SearchActivityKind::X,
        /*animations_enabled*/ true,
    );
    assert!(cell.transcript_animation_tick().is_some());
    insta::assert_snapshot!(cell.raw_lines()[0].to_string(), @"Searching X");
    cell.complete();
    assert_eq!(cell.transcript_animation_tick(), None);
    insta::assert_snapshot!(cell.display_lines(/*width*/ 80)[0].to_string(), @"• X search completed");
    insta::assert_snapshot!(cell.transcript_lines(/*width*/ 12).iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"), @"
    • X search
      completed
    ");
}
