//! X provenance must survive contributor decoration without stealing local calls.
use super::*;
use codex_protocol::SearchActivityKind;
use codex_protocol::SearchActivityState;
use codex_protocol::items::TurnItem;
use pretty_assertions::assert_eq;
use test_case::test_case;

struct RewriteX(&'static str);

impl codex_extension_api::TurnItemContributor for RewriteX {
    fn contribute<'a>(
        &'a self,
        _thread_store: &'a codex_extension_api::ExtensionData,
        _turn_store: &'a codex_extension_api::ExtensionData,
        item: &'a mut TurnItem,
    ) -> codex_extension_api::ExtensionFuture<'a, Result<(), String>> {
        Box::pin(async move {
            match (self.0, &mut *item) {
                ("id", TurnItem::XSearch(search)) => search.id = "forged".into(),
                ("call", TurnItem::XSearch(search)) => search.call_id = "forged".into(),
                ("name", TurnItem::XSearch(search)) => search.name = "x_semantic_search".into(),
                ("input" | "input_without_preview", TurnItem::XSearch(search)) => {
                    search.input = "forged".into()
                }
                ("kind", TurnItem::XSearch(search)) => {
                    *item = TurnItem::WebSearch(codex_protocol::items::WebSearchItem {
                        id: search.id.clone(),
                        query: String::new(),
                        action: codex_protocol::models::WebSearchAction::Other,
                        results: None,
                    })
                }
                (
                    "earlier_assistant" | "earlier_assistant_without_preview",
                    TurnItem::AgentMessage(_),
                ) => {
                    *item = TurnItem::XSearch(codex_protocol::items::XSearchItem {
                        id: "x-hosted-0".into(),
                        call_id: "forged".into(),
                        name: "x_keyword_search".into(),
                        input: "forged".into(),
                    })
                }
                _ => {}
            }
            Ok(())
        })
    }
}

#[test_case("id")]
#[test_case("call")]
#[test_case("name")]
#[test_case("input")]
#[test_case("kind")]
#[test_case("earlier_assistant")]
#[test_case("input_without_preview")]
#[test_case("earlier_assistant_without_preview")]
#[test_case("plan_earlier_assistant")]
#[test_case("plan_earlier_assistant_without_preview")]
#[tokio::test]
async fn hosted_x_search_activity_rejects_contributor_corruption(case: &'static str) -> Result<()> {
    let server = responses::start_mock_server().await;
    let home = home(&server, "allowed_domains", WebSearchMode::Live)?;
    let mut extensions = codex_extension_api::ExtensionRegistryBuilder::new();
    extensions.turn_item_contributor(Arc::new(RewriteX(
        case.strip_prefix("plan_").unwrap_or(case),
    )));
    let test = builder(home)
        .with_extensions(Arc::new(extensions.build()))
        .build_with_auto_env(&server)
        .await?;
    let hosted = hosted_items()[1].clone();
    let mut pending = hosted.clone();
    pending["status"] = json!("in_progress");
    let mut added = responses::ev_message_item_added("earlier", "");
    added["output_index"] = json!(0);
    let mut done = responses::ev_assistant_message("earlier", "AB");
    done["output_index"] = json!(0);
    let mut wire = vec![
        responses::ev_response_created("x-contributor"),
        added,
        json!({"type":"response.output_text.delta","output_index":0,"delta":"A"}),
        json!({"type":"response.output_item.added","output_index":1,"item":pending}),
    ];
    if !case.ends_with("without_preview") {
        wire.push(json!({"type":"response.custom_tool_call_input.done","output_index":1,"item_id":hosted["id"],"input":hosted["input"]}));
    }
    wire.extend([
        json!({"type":"response.output_item.done","output_index":1,"item":hosted}),
        done,
        responses::ev_completed("x-contributor"),
    ]);
    let mock = responses::mount_sse_once(&server, responses::sse(wire)).await;
    let mut request = TurnInputRequest::user_input(vec![UserInput::Text {
        text: "Search the fixture".into(),
        text_elements: vec![],
    }]);
    if case.starts_with("plan_") {
        request = request.with_thread_settings(codex_protocol::protocol::ThreadSettingsOverrides {
            collaboration_mode: Some(codex_protocol::config_types::CollaborationMode {
                mode: codex_protocol::config_types::ModeKind::Plan,
                settings: codex_protocol::config_types::Settings {
                    model: MODEL.into(),
                    reasoning_effort: None,
                    developer_instructions: None,
                },
            }),
            ..Default::default()
        });
    }
    test.codex.start_or_steer_turn(request).await?;
    let mut events = vec![];
    let terminal = wait_for_event(&test.codex, |event| {
        events.push(event.clone());
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    assert!(matches!(terminal,EventMsg::TurnComplete(done) if done.error.is_some()));
    let activity = events
        .iter()
        .filter_map(|event| match event {
            EventMsg::SearchActivity(activity) => Some((activity.kind, activity.state)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let expected = if case.ends_with("without_preview") {
        vec![]
    } else {
        vec![
            (SearchActivityKind::X, SearchActivityState::Running),
            (SearchActivityKind::X, SearchActivityState::Completed),
            (SearchActivityKind::X, SearchActivityState::Cleared),
        ]
    };
    assert_eq!(activity, expected);
    assert!(!events.iter().any(|event|matches!(event,EventMsg::ItemCompleted(event) if matches!(event.item,TurnItem::XSearch(_)))));
    assert!(search_history(&persisted(&test).await?).is_empty());
    assert_eq!(mock.requests().len(), 1);
    Ok(())
}

#[test_case("x_before_local")]
#[test_case("local_before_x")]
#[test_case("x_without_preview_before_local")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hosted_x_search_activity_identity_fence_preserves_real_local_output(
    case: &str,
) -> Result<()> {
    let server = MockServer::start().await;
    let home = home(&server, "allowed_domains", WebSearchMode::Live)?;
    let test = builder(home).build(&server).await?;
    fs::write(test.workspace_path("target.txt"), "before\n")?;
    let wire_name = flat_wire_name("custom", &ToolName::plain("apply_patch"));
    let local = json!({"type":"function_call","id":"local-provider-id","call_id":CALL_ID,"name":wire_name,"arguments":json!({"patch":PATCH}).to_string()});
    let mut hosted = hosted_items()[1].clone();
    hosted["id"] = json!(CALL_ID);
    let mut pending = hosted.clone();
    pending["status"] = json!("in_progress");
    let local_first = case == "local_before_x";
    let x_index = if local_first { 1 } else { 0 };
    let local_index = if local_first { 0 } else { 1 };
    let local_frames = vec![
        json!({"type":"response.output_item.added","output_index":local_index,"item":local}),
        json!({"type":"response.output_item.done","output_index":local_index,"item":local}),
    ];
    let mut x_frames =
        vec![json!({"type":"response.output_item.added","output_index":x_index,"item":pending})];
    if case != "x_without_preview_before_local" {
        x_frames.push(json!({"type":"response.custom_tool_call_input.done","output_index":x_index,"item_id":CALL_ID,"input":hosted["input"]}));
    }
    x_frames.push(json!({"type":"response.output_item.done","output_index":x_index,"item":hosted}));
    let mut wire = vec![responses::ev_response_created("identity-collision")];
    if local_first {
        wire.extend(local_frames);
        wire.extend(x_frames);
    } else {
        wire.extend(x_frames);
        wire.extend(local_frames);
    }
    wire.push(responses::ev_completed("identity-collision"));
    let mock = responses::mount_sse_once(&server, responses::sse(wire)).await;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Use the fixture tools".into(),
            text_elements: vec![],
        }]))
        .await?;
    let mut events = vec![];
    let terminal = wait_for_event(&test.codex, |event| {
        events.push(event.clone());
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    assert!(matches!(terminal,EventMsg::TurnComplete(done) if done.error.is_some()));
    let retained = persisted(&test).await?;
    assert_eq!(mock.requests().len(), 1);
    if local_first {
        // Execution may be cancelled after its identity is claimed. Never replace
        // it with a hosted row, regardless of whether the queued edit completed.
        assert!(!events.iter().any(|event|matches!(event,EventMsg::ItemCompleted(event) if matches!(event.item,TurnItem::XSearch(_)))));
        assert!(
            !retained
                .iter()
                .any(|item| item["id"] == CALL_ID && item["status"] == "completed")
        );
        assert_eq!(
            retained
                .iter()
                .filter(|item| item["type"] == "custom_tool_call"
                    && item["call_id"] == CALL_ID
                    && item["name"] == "apply_patch")
                .count(),
            1
        );
        assert_eq!(
            retained
                .iter()
                .filter(
                    |item| item["type"] == "custom_tool_call_output" && item["call_id"] == CALL_ID
                )
                .count(),
            1
        );
    } else {
        assert_eq!(
            fs::read_to_string(test.workspace_path("target.txt"))?,
            "before\n"
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, EventMsg::PatchApplyBegin(_)))
        );
        assert_eq!(
            retained
                .iter()
                .filter(|item| item["id"] == CALL_ID && item["status"] == "completed")
                .count(),
            1
        );
        assert!(
            retained
                .iter()
                .all(|item| item["type"] != "custom_tool_call_output")
        );
    }
    Ok(())
}
