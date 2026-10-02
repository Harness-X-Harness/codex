use codex_core::NotSubmittedReason;
use codex_core::SteerSubmission;
use codex_core::TurnInput;
use codex_core::TurnInputRequest;
use codex_core::TurnInputSubmission;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStartInput;
use codex_extension_api::TurnStopInput;
use codex_features::Feature;
use codex_history::InitialHistory;
use codex_history::ResumedHistory;
use codex_history::RolloutItem;
use codex_protocol::items::TurnItem;
use codex_protocol::mcp::ClientMcpExtensions;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::AdditionalContextEntry;
use codex_protocol::protocol::AdditionalContextKind;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::user_input::ByteRange;
use codex_protocol::user_input::TextElement;
use codex_protocol::user_input::UserInput;
use codex_thread_store::InMemoryThreadStore;
use codex_thread_store::LoadThreadHistoryParams;
use core_test_support::ThreadIdle;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use test_case::test_case;
use tokio::sync::oneshot;
use tokio::time::timeout;

const GATE_TIMEOUT: Duration = Duration::from_secs(30);

struct Gate {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

impl Gate {
    async fn wait(self) {
        self.entered.send(()).expect("test is waiting for the gate");
        timeout(GATE_TIMEOUT, self.release)
            .await
            .expect("test should release the lifecycle gate")
            .expect("release sender should remain alive");
    }
}

// on_turn_stop runs after the real final input snapshot and history recording,
// but before the terminal event and active-slot clearance. Only the first stop
// is held. The optional second-start gate exposes a replacement's reserved slot.
struct CompletionWindow {
    stop: Mutex<Option<Gate>>,
    replacement_start: Mutex<Option<Gate>>,
    starts: AtomicUsize,
}

impl TurnLifecycleContributor for CompletionWindow {
    fn on_turn_start<'a>(&'a self, _input: TurnStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if self.starts.fetch_add(1, Ordering::SeqCst) == 1 {
                let gate = self.replacement_start.lock().unwrap().take();
                if let Some(gate) = gate {
                    gate.wait().await;
                }
            }
        })
    }

    fn on_turn_stop<'a>(&'a self, _input: TurnStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let gate = self.stop.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.wait().await;
            }
        })
    }
}

fn text_input(text: &str) -> TurnInputRequest {
    TurnInputRequest::user_input(vec![UserInput::Text {
        text: text.to_string(),
        text_elements: Vec::new(),
    }])
}

fn semantic_item(item: ResponseItem) -> ResponseItem {
    let mut item = responses::strip_metadata(item);
    if let ResponseItem::Message { id, .. } = &mut item {
        *id = None;
    }
    item
}

fn recorded_client_items(
    history: &[RolloutItem],
    expected: &[ResponseItem],
) -> Vec<(ResponseItem, bool, bool)> {
    history
        .iter()
        .filter_map(|item| {
            let RolloutItem::ResponseItem(envelope) = item else {
                return None;
            };
            let item = semantic_item(envelope.item.clone());
            expected.contains(&item).then(|| {
                let metadata = envelope.metadata.as_ref();
                (
                    item,
                    metadata.is_some_and(|metadata| metadata.client_authored),
                    metadata.is_some_and(|metadata| metadata.harness_authored_configuration),
                )
            })
        })
        .collect()
}

#[test_case(ThreadHistoryMode::Legacy; "legacy")]
#[test_case(ThreadHistoryMode::Paginated; "paginated")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closed_completion_redirects_client_items_and_survives_resume(
    history_mode: ThreadHistoryMode,
) -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let initial_response =
        responses::mount_sse_once(&server, responses::sse_completed("initial")).await;
    let (entered, reached_stop) = oneshot::channel();
    let (release_stop, release) = oneshot::channel();
    let window = Arc::new(CompletionWindow {
        stop: Mutex::new(Some(Gate { entered, release })),
        replacement_start: Mutex::new(None),
        starts: AtomicUsize::new(0),
    });
    let mut extensions = ExtensionRegistryBuilder::new();
    extensions.turn_lifecycle_contributor(window.clone());
    extensions.thread_lifecycle_contributor(Arc::new(ThreadIdle));
    let test = test_codex()
        .with_history_mode(history_mode)
        .with_extensions(Arc::new(extensions.build()))
        .with_config(|config| {
            config
                .features
                .enable(Feature::RetainClientDeveloperMessages)
                .unwrap();
        })
        .build_with_auto_env(&server)
        .await?;
    let TurnInputSubmission::Started { turn_id } = test
        .codex
        .start_or_steer_turn(text_input("finish the initial turn"))
        .await?
    else {
        panic!("initial input should start a turn");
    };
    timeout(GATE_TIMEOUT, reached_stop).await??;

    // This public bridge is also used by goal-extension active-turn advice.
    let rejected = vec![
        responses::user_message_item("rejected generic item one"),
        responses::user_message_item("rejected generic item two"),
    ];
    assert_eq!(
        test.codex.inject_if_running(rejected.clone()).await,
        Err(rejected),
    );
    assert_eq!(
        test.codex
            .steer_turn(text_input("rejected late steer"), turn_id.clone())
            .await?,
        SteerSubmission::NotSubmitted {
            reason: NotSubmittedReason::NoActiveTurn
        },
    );
    let developer = |text: &str| ResponseItem::Message {
        id: None,
        role: "developer".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };
    let client_items = vec![
        responses::user_message_item("redirected client user context"),
        developer("redirected client developer context"),
        developer("<image_resize_notice>client marker-shaped context</image_resize_notice>"),
    ];
    let expected_recorded = vec![
        (client_items[0].clone(), false, false),
        (client_items[1].clone(), true, false),
        (client_items[2].clone(), true, false),
    ];
    test.codex.inject_response_items(client_items.clone()).await?;
    let recorded = test.codex.load_history(/*include_archived*/ true).await?;
    assert_eq!(
        recorded_client_items(&recorded.items, &client_items),
        expected_recorded
    );

    let mut raw_items = Vec::new();
    let mut started_events = 0;
    wait_for_event(&test.codex, |event| {
        assert!(
            !matches!(event, EventMsg::TurnComplete(_)),
            "completion must remain held"
        );
        if matches!(event, EventMsg::TurnStarted(_)) {
            started_events += 1;
        }
        if let EventMsg::RawResponseItem(event) = event {
            let item = semantic_item(event.item.clone());
            if client_items.contains(&item) {
                raw_items.push(item);
            }
        }
        raw_items.len() == client_items.len()
    })
    .await;
    assert_eq!(raw_items, client_items);
    assert_eq!(started_events, 1);
    assert_eq!(window.starts.load(Ordering::SeqCst), 1);
    assert_eq!(initial_response.requests().len(), 1);
    release_stop.send(()).expect("normal completion is held");
    wait_for_event(&test.codex, |event| {
        assert!(
            !matches!(event, EventMsg::TurnStarted(_)),
            "history redirection cannot start a turn"
        );
        matches!(event, EventMsg::TurnComplete(event) if event.turn_id == turn_id)
    })
    .await;
    ThreadIdle::wait(&test.codex).await;
    test.codex.shutdown_and_wait().await?;

    let history = test
        .thread_store
        .load_latest_model_context(LoadThreadHistoryParams {
            thread_id: test.session_configured.thread_id,
            include_archived: true,
        })
        .await?;
    assert_eq!(
        recorded_client_items(&history.items, &client_items),
        expected_recorded
    );
    let resumed = test
        .thread_manager
        .resume_thread_with_history(
            test.config.clone(),
            InitialHistory::Resumed(ResumedHistory {
                conversation_id: history.thread_id,
                history: Arc::new(history.items),
                rollout_path: test.session_configured.rollout_path.clone(),
            }),
            test.thread_manager.auth_manager(),
            /*parent_trace*/ None,
            ClientMcpExtensions::default(),
        )
        .await?;
    assert_eq!(
        resumed.thread.config_snapshot().await.history_mode,
        history_mode
    );
    let next_response =
        responses::mount_sse_once(&server, responses::sse_completed("resumed")).await;
    resumed
        .thread
        .start_or_steer_turn(text_input("continue after resume"))
        .await?;
    wait_for_event(&resumed.thread, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    ThreadIdle::wait(&resumed.thread).await;
    let request = next_response.single_request();
    for (role, text) in [
        ("user", "redirected client user context"),
        ("developer", "redirected client developer context"),
        (
            "developer",
            "<image_resize_notice>client marker-shaped context</image_resize_notice>",
        ),
    ] {
        assert_eq!(
            request
                .message_input_texts(role)
                .iter()
                .filter(|value| value.as_str() == text)
                .count(),
            1
        );
    }
    for text in [
        "rejected generic item one",
        "rejected generic item two",
        "rejected late steer",
    ] {
        assert!(!request.body_contains_text(text));
    }
    assert!(request.input().iter().all(|item| {
        item.get("metadata").is_none() && item.get("client_authored").is_none()
    }));
    assert_eq!(window.starts.load(Ordering::SeqCst), 2);
    resumed.thread.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn start_or_steer_preserves_payload_while_old_completion_finishes() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let initial_response =
        responses::mount_sse_once(&server, responses::sse_completed("old-turn")).await;
    let (entered, reached_stop) = oneshot::channel();
    let (release_stop, release) = oneshot::channel();
    let (replacement_entered, reached_replacement) = oneshot::channel();
    let (release_replacement, replacement_release) = oneshot::channel();
    let window = Arc::new(CompletionWindow {
        stop: Mutex::new(Some(Gate { entered, release })),
        replacement_start: Mutex::new(Some(Gate {
            entered: replacement_entered,
            release: replacement_release,
        })),
        starts: AtomicUsize::new(0),
    });
    let mut extensions = ExtensionRegistryBuilder::new();
    extensions.turn_lifecycle_contributor(window.clone());
    extensions.thread_lifecycle_contributor(Arc::new(ThreadIdle));
    let store = Arc::new(InMemoryThreadStore::default());
    let test = test_codex()
        .with_thread_store(store.clone())
        .with_extensions(Arc::new(extensions.build()))
        .build_with_auto_env(&server)
        .await?;
    let TurnInputSubmission::Started {
        turn_id: old_turn_id,
    } = test
        .codex
        .start_or_steer_turn(text_input("complete before replacement"))
        .await?
    else {
        panic!("old turn should start");
    };
    timeout(GATE_TIMEOUT, reached_stop).await??;
    let next_response = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse_completed("replacement-input"),
            responses::sse_completed("replacement-pending-input"),
        ],
    )
    .await;
    let content = vec![
        UserInput::Text {
            text: "first replacement text".to_string(),
            text_elements: vec![TextElement::new(
                ByteRange { start: 0, end: 5 },
                Some("first".to_string()),
            )],
        },
        UserInput::Text {
            text: "second replacement text".to_string(),
            text_elements: Vec::new(),
        },
    ];
    let request = TurnInputRequest::new(TurnInput::UserInput {
        content: content.clone(),
        client_id: Some("replacement-client-message".to_string()),
    })
    .with_additional_context(BTreeMap::from([
        (
            "application_context".to_string(),
            AdditionalContextEntry {
                value: "replacement application context".to_string(),
                kind: AdditionalContextKind::Application,
            },
        ),
        (
            "untrusted_context".to_string(),
            AdditionalContextEntry {
                value: "replacement untrusted context".to_string(),
                kind: AdditionalContextKind::Untrusted,
            },
        ),
    ]))
    .with_responses_metadata(Some(HashMap::from([(
        "completion_test_client".to_string(),
        "replacement metadata".to_string(),
    )])));
    let replacement = tokio::spawn({
        let codex = Arc::clone(&test.codex);
        async move { codex.start_or_steer_turn(request).await }
    });
    timeout(GATE_TIMEOUT, reached_replacement).await??;
    let flushes_before_release = store.calls().await.flush_thread;
    release_stop.send(()).expect("old completion is held");
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(event) if event.turn_id == old_turn_id)
    })
    .await;
    // The replacement is held before task registration, so it cannot flush.
    // The old terminal flush happens after the Arc-checked slot clearance.
    // Observe that operation, not elapsed time, before probing the reserved slot.
    timeout(GATE_TIMEOUT, async {
        while store.calls().await.flush_thread == flushes_before_release {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    let reserved_item = responses::user_message_item("accepted into replacement reservation");
    assert_eq!(
        test.codex.inject_if_running(vec![reserved_item]).await,
        Ok(())
    );
    assert_eq!(window.starts.load(Ordering::SeqCst), 2);
    assert!(next_response.requests().is_empty());
    release_replacement
        .send(())
        .expect("replacement registration is held");
    let TurnInputSubmission::Started {
        turn_id: new_turn_id,
    } = timeout(GATE_TIMEOUT, replacement).await???
    else {
        panic!("StartOrSteer must create the replacement turn");
    };
    assert_ne!(new_turn_id, old_turn_id);
    let mut user_items = Vec::new();
    wait_for_event(&test.codex, |event| {
        if let EventMsg::ItemCompleted(event) = event
            && let TurnItem::UserMessage(item) = &event.item
            && item.client_id.as_deref() == Some("replacement-client-message")
        {
            user_items.push((item.content.clone(), item.client_id.clone()));
        }
        matches!(event, EventMsg::TurnComplete(event) if event.turn_id == new_turn_id)
    })
    .await;
    ThreadIdle::wait(&test.codex).await;
    assert_eq!(
        user_items,
        vec![(content, Some("replacement-client-message".to_string()))]
    );
    // Explicit start input owns the first sampling step; the reserved queue is
    // drained for its follow-up step, without creating another turn.
    let requests = next_response.requests();
    assert_eq!(requests.len(), 2);
    let request = &requests[0];
    for text in ["first replacement text", "second replacement text"] {
        assert!(request.body_contains_text(text));
    }
    assert!(!request.body_contains_text("accepted into replacement reservation"));
    assert_eq!(
        requests[1]
            .message_input_texts("user")
            .iter()
            .filter(|text| text.as_str() == "accepted into replacement reservation")
            .count(),
        1
    );
    assert!(
        request
            .message_input_texts("developer")
            .iter()
            .any(|text| text.contains("replacement application context"))
    );
    assert!(
        request
            .message_input_texts("user")
            .iter()
            .any(|text| text.contains("replacement untrusted context"))
    );
    let body = request.body_json();
    let turn_metadata: serde_json::Value = serde_json::from_str(
        body["client_metadata"]["x-codex-turn-metadata"]
            .as_str()
            .expect("canonical turn metadata"),
    )?;
    assert_eq!(turn_metadata["completion_test_client"], "replacement metadata");
    assert_eq!(window.starts.load(Ordering::SeqCst), 2);
    assert_eq!(initial_response.requests().len(), 1);
    test.codex.shutdown_and_wait().await?;
    Ok(())
}
