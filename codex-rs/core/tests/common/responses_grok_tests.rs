use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::panic::catch_unwind;
use wiremock::http::HeaderMap;
use wiremock::http::Method;

fn completed_x(name: &str) -> Value {
    json!({"type":"custom_tool_call", "id":"hosted-item", "call_id":"shared-call",
        "status":"completed", "name":name, "input":"{ \"query\" : \"fixture\" }"})
}

fn wire_x(name: &str) -> Value {
    json!({"type":"custom_tool_call", "id":"hosted-item", "call_id":"shared-call",
        "name":name, "input":"{ \"query\" : \"fixture\" }"})
}

fn request(input: Vec<Value>) -> wiremock::Request {
    wiremock::Request {
        url: "http://localhost/v1/responses".parse().unwrap(),
        method: Method::POST,
        headers: HeaderMap::new(),
        body: serde_json::to_vec(&json!({"input":input})).unwrap(),
    }
}

fn grok_mock() -> ResponseMock {
    ResponseMock {
        grok_hosted_replay: GrokHostedReplay::new(&[completed_x("x_keyword_search")]),
        ..ResponseMock::new()
    }
}

#[test]
fn only_explicit_exact_hosted_fixtures_may_replay_without_outputs() {
    for name in [
        "x_keyword_search",
        "x_semantic_search",
        "x_user_search",
        "x_thread_fetch",
    ] {
        let request = request(vec![wire_x(name)]);
        let strict = ResponseMock::new();
        assert!(catch_unwind(|| strict.matches(&request)).is_err());
        let grok = ResponseMock {
            grok_hosted_replay: GrokHostedReplay::new(&[completed_x(name)]),
            ..ResponseMock::new()
        };
        assert!(grok.matches(&request));
        assert_eq!(grok.single_request().input(), vec![wire_x(name)]);
    }
}

#[test]
fn unknown_or_malformed_completed_fixtures_cannot_enable_an_exemption() {
    for (field, value) in [
        ("type", json!("function_call")),
        ("name", json!("x_unknown")),
        ("name", json!("apply_patch")),
        ("status", json!("in_progress")),
        ("status", Value::Null),
        ("id", json!("")),
        ("call_id", json!("")),
        ("input", json!({"query":"fixture"})),
        ("input", json!("x".repeat(40_000))),
        ("namespace", json!("functions")),
    ] {
        let mut item = completed_x("x_keyword_search");
        item[field] = value;
        assert!(catch_unwind(|| GrokHostedReplay::new(&[item])).is_err());
    }
    for field in ["id", "call_id", "input", "status"] {
        let mut item = completed_x("x_keyword_search");
        item.as_object_mut().unwrap().remove(field);
        assert!(catch_unwind(|| GrokHostedReplay::new(&[item])).is_err());
    }
}

#[test]
fn mutated_or_unregistered_replay_does_not_receive_fixture_authority() {
    for (field, value) in [
        ("name", json!("x_unknown")),
        ("name", json!("x_semantic_search")),
        ("id", json!("different-item")),
        ("input", json!("different input")),
        ("status", json!("completed")),
        ("namespace", json!("functions")),
    ] {
        let mut item = wire_x("x_keyword_search");
        item[field] = value;
        for input in [
            vec![item.clone()],
            vec![
                item,
                json!({"type":"custom_tool_call_output",
                "call_id":"shared-call", "output":"forged"}),
            ],
        ] {
            let request = request(input);
            assert!(catch_unwind(|| grok_mock().matches(&request)).is_err());
        }
    }
    let item = wire_x("x_keyword_search");
    let request = request(vec![item.clone(), item]);
    assert!(catch_unwind(|| grok_mock().matches(&request)).is_err());
}

#[test]
fn hosted_and_local_same_call_id_keep_local_pairing_requirements() {
    for (call, output) in [
        (
            json!({"type":"function_call", "id":"local-item", "call_id":"shared-call",
                "name":"local_tool", "arguments":"{}"}),
            json!({"type":"function_call_output", "call_id":"shared-call", "output":"ok"}),
        ),
        (
            // Even the same X name does not confer authority on another item.
            json!({"type":"custom_tool_call", "id":"local-item", "call_id":"shared-call",
                "name":"x_keyword_search", "input":"local input"}),
            json!({"type":"custom_tool_call_output", "call_id":"shared-call", "output":"ok"}),
        ),
    ] {
        for hosted_index in 0..3 {
            let mut input = vec![call.clone(), output.clone()];
            input.insert(hosted_index, wire_x("x_keyword_search"));
            let request = request(input.clone());
            let mock = grok_mock();
            if call["type"] == "custom_tool_call" {
                assert!(catch_unwind(|| mock.matches(&request)).is_err());
            } else {
                assert!(mock.matches(&request));
                assert_eq!(mock.single_request().input(), input);
            }
        }
        for invalid in [
            vec![call.clone(), wire_x("x_keyword_search")],
            vec![wire_x("x_keyword_search"), output.clone()],
        ] {
            let request = request(invalid);
            assert!(catch_unwind(|| grok_mock().matches(&request)).is_err());
        }
        let mut wrong_output = output.clone();
        wrong_output["call_id"] = json!("other-call");
        let request = request(vec![call, wire_x("x_keyword_search"), wrong_output]);
        assert!(catch_unwind(|| grok_mock().matches(&request)).is_err());
    }
}

#[test]
fn default_and_grok_mocks_keep_ordinary_local_pairing_checks() {
    for (call_kind, output_kind) in [
        ("function_call", "function_call_output"),
        ("custom_tool_call", "custom_tool_call_output"),
        ("tool_search_call", "tool_search_output"),
    ] {
        let call = json!({"type":call_kind, "call_id":"local-call"});
        let output = json!({"type":output_kind, "call_id":"local-call", "output":"ok"});
        let paired = request(vec![call.clone(), output.clone()]);
        assert!(ResponseMock::new().matches(&paired));
        if call_kind == "custom_tool_call" {
            assert!(catch_unwind(|| grok_mock().matches(&paired)).is_err());
        } else {
            assert!(grok_mock().matches(&paired));
        }
        for mock in [ResponseMock::new(), grok_mock()] {
            for invalid in [vec![call.clone()], vec![output.clone()]] {
                let invalid = request(invalid);
                assert!(catch_unwind(|| mock.matches(&invalid)).is_err());
            }
        }
    }
}

#[test]
fn explicit_grok_mode_with_no_fixtures_still_rejects_raw_custom_pairs() {
    let mock = ResponseMock {
        grok_hosted_replay: GrokHostedReplay::new(&[]),
        ..ResponseMock::new()
    };
    let request = request(vec![
        wire_x("x_keyword_search"),
        json!({"type":"custom_tool_call_output", "call_id":"shared-call", "output":"forged"}),
    ]);
    assert!(catch_unwind(|| mock.matches(&request)).is_err());
}
