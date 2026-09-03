//! Model-visible request snapshots for the Grok graft.
//!
//! Each Live Story's first Turn is replayed against a mock Grok gateway and
//! the `/responses` request grok-4.6 would see is rendered in a compact,
//! digest-based form: model and reasoning fields, the tool list with a
//! description digest per tool, the instruction digest, and the input item
//! kinds. An upstream bump that changes what the model sees (a tool renamed,
//! a shell type collapsed, an instruction block added) shows up here as a
//! snapshot diff at PR time instead of as a Live deadline expiry.
//!
//! Kept apart from the stock suite so upstream edits never collide with the
//! Grok graft.

use std::sync::Arc;

use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_core::config::Config;
use codex_extension_api::ExtensionRegistry;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_features::Feature;
use codex_image_generation_extension::install as install_image_generation_extension;
use codex_login::CodexAuth;
use codex_model_provider_info::WireApi;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use core_test_support::context_snapshot;
use core_test_support::context_snapshot::ContextSnapshotOptions;
use core_test_support::context_snapshot::ContextSnapshotRenderMode;
use core_test_support::responses::ResponsesRequest;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::test_codex::TestCodexBuilder;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use sha2::Digest;
use sha2::Sha256;

// The first-Turn prompts of grokex/live_contracts.json scenarios, verbatim.
const BASIC_PROMPT: &str = "Reply with exactly GROKEX_BASIC_RESPONSE_OK and no other text.";
const CONTINUATION_PROMPT: &str = "Use the grokex_live_probe result, then reply with exactly GROKEX_LIVE_RESPONSE_OK and no other text.";
const COLLABORATION_PROMPT: &str = "Delegate one bounded task to a child named live_child using the default \
full-history fork. Tell the child: Generate a fresh UUID v4 and reply with exactly its canonical lowercase \
text and no other text. Wait for that child to complete, then reply with exactly the UUID returned by the \
child and no other text.";
const IMAGE_PROMPT: &str = "Generate an image of a blue circle on a plain white background.";

/// The packaged Grokex registers the image-generation extension, so the tool
/// inventory the model sees includes it; core tests must install it explicitly.
fn grok_extensions(auth: &CodexAuth) -> Arc<ExtensionRegistry<Config>> {
    let auth_manager = codex_core::test_support::auth_manager_from_auth(auth.clone());
    let mut extensions = ExtensionRegistryBuilder::<Config>::new();
    install_image_generation_extension(&mut extensions, auth_manager, |config: &Config| {
        Some(config.codex_home.clone())
    });
    Arc::new(extensions.build())
}

fn grok_builder() -> TestCodexBuilder {
    let auth = CodexAuth::create_dummy_chatgpt_auth_for_testing();
    let extensions = grok_extensions(&auth);
    test_codex()
        .with_auth(auth)
        .with_extensions(extensions)
        .with_model("grok-4.6")
        .with_config(|config| {
            config.model_provider_id = "grok".to_string();
            config.model_provider.name = "Grok".to_string();
            config.model_provider.wire_api = WireApi::GrokResponses;
            config.model_provider.requires_openai_auth = false;
        })
}

fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn first_line(text: &str, max_chars: usize) -> String {
    let line = text.lines().next().unwrap_or_default();
    if line.chars().count() <= max_chars {
        return line.to_string();
    }
    let prefix: String = line.chars().take(max_chars).collect();
    format!("{prefix}…")
}

/// Renders the request the way a reviewer needs to compare it across upstream
/// bumps: every top-level field, the exact tool inventory with per-tool
/// description digests, the instruction digest, and the input item kinds with
/// a short text prefix. Prompts and instructions never appear in full.
fn render_model_visible_request(scenario: &str, request: &ResponsesRequest) -> String {
    let body = request.body_json();
    let mut out = format!("Scenario: {scenario}\n\n## Request\n");
    let mut fields: Vec<&String> = body
        .as_object()
        .map(|object| object.keys().collect())
        .unwrap_or_default();
    fields.sort();
    for field in fields {
        let value = &body[field];
        match field.as_str() {
            "input" | "tools" => {}
            // Per-run identifiers: keep the shape, drop the values.
            "client_metadata" => {
                let mut keys: Vec<&String> = value
                    .as_object()
                    .map(|object| object.keys().collect())
                    .unwrap_or_default();
                keys.sort();
                out.push_str(&format!(
                    "client_metadata: keys=[{}]\n",
                    keys.iter()
                        .map(|key| key.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
            "prompt_cache_key" => out.push_str("prompt_cache_key: <thread id>\n"),
            "instructions" => {
                let text = value.as_str().unwrap_or_default();
                out.push_str(&format!(
                    "instructions: sha256={} chars={} first_line={:?}\n",
                    digest(text),
                    text.chars().count(),
                    first_line(text, 72)
                ));
            }
            _ => out.push_str(&format!("{field}: {value}\n")),
        }
    }

    let tools = body["tools"].as_array().cloned().unwrap_or_default();
    out.push_str(&format!("\n## Tools ({})\n", tools.len()));
    for tool in &tools {
        let kind = tool["type"].as_str().unwrap_or("?");
        let name = tool["name"].as_str().unwrap_or("-");
        let description = tool["description"].as_str().unwrap_or_default();
        let parameters = tool
            .get("parameters")
            .map(std::string::ToString::to_string)
            .unwrap_or_default();
        let mut properties: Vec<&String> = tool["parameters"]["properties"]
            .as_object()
            .map(|object| object.keys().collect())
            .unwrap_or_default();
        properties.sort();
        let extra: Vec<String> = tool
            .as_object()
            .map(|object| {
                object
                    .keys()
                    .filter(|key| {
                        !matches!(key.as_str(), "type" | "name" | "description" | "parameters")
                    })
                    .map(|key| format!("{key}={}", tool[key]))
                    .collect()
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "- {kind} {name} description=sha256:{}/{}chars params=sha256:{} props=[{}]{}\n  {:?}\n",
            digest(description),
            description.chars().count(),
            digest(&parameters),
            properties
                .iter()
                .map(|property| property.as_str())
                .collect::<Vec<_>>()
                .join(","),
            if extra.is_empty() {
                String::new()
            } else {
                format!(" {}", extra.join(" "))
            },
            first_line(description, 96),
        ));
    }

    out.push_str("\n## Input\n");
    out.push_str(&context_snapshot::format_request_input_snapshot(
        request,
        &ContextSnapshotOptions::default()
            .render_mode(ContextSnapshotRenderMode::KindWithTextPrefix { max_chars: 72 })
            .strip_response_item_ids(),
    ));
    out
}

async fn capture_first_request(
    builder: TestCodexBuilder,
    prompt: &str,
) -> Result<ResponsesRequest> {
    let server = start_mock_server().await;
    let mock = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-1"),
            ev_assistant_message("msg-1", "ok"),
            ev_completed("resp-1"),
        ]),
    )
    .await;
    let mut builder = builder;
    let test = builder.build(&server).await?;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: prompt.to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_event(test.codex.as_ref(), |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    Ok(mock.single_request())
}

fn assert_grok_request(request: &ResponsesRequest) -> Result<()> {
    let body = request.body_json();
    anyhow::ensure!(
        body["model"] == "grok-4.6",
        "request model should be grok-4.6"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn snapshot_grok_model_visible_request_basic() -> Result<()> {
    let request = capture_first_request(grok_builder(), BASIC_PROMPT).await?;
    assert_grok_request(&request)?;
    insta::assert_snapshot!(
        "grok_model_visible_request_basic",
        render_model_visible_request("basic-exact-reply first Turn", &request)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn snapshot_grok_model_visible_request_continuation() -> Result<()> {
    let request = capture_first_request(grok_builder(), CONTINUATION_PROMPT).await?;
    assert_grok_request(&request)?;
    insta::assert_snapshot!(
        "grok_model_visible_request_continuation",
        render_model_visible_request(
            "encrypted-reasoning-tool-continuation first Turn (dynamic tool declared at thread/start is not part of this fixture)",
            &request
        )
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn snapshot_grok_model_visible_request_collaboration_ultra() -> Result<()> {
    let builder = grok_builder().with_config(|config| {
        config
            .features
            .enable(Feature::Collab)
            .expect("test config should allow feature update");
        config.model_reasoning_effort = Some(ReasoningEffort::Ultra);
    });
    let request = capture_first_request(builder, COLLABORATION_PROMPT).await?;
    assert_grok_request(&request)?;
    insta::assert_snapshot!(
        "grok_model_visible_request_collaboration_ultra",
        render_model_visible_request("ultra-full-history-collaboration first Turn", &request)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn snapshot_grok_model_visible_request_image() -> Result<()> {
    let request = capture_first_request(grok_builder(), IMAGE_PROMPT).await?;
    assert_grok_request(&request)?;
    insta::assert_snapshot!(
        "grok_model_visible_request_image",
        render_model_visible_request("image-generation-history-edit first Turn", &request)
    );
    Ok(())
}
