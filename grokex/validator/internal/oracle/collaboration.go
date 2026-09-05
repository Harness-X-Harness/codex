package oracle

import (
	"regexp"
	"strings"
	"time"

	"github.com/Harness-X-Harness/codex/grokex/validator/internal/driver"
	"github.com/Harness-X-Harness/codex/grokex/validator/internal/rollout"
)

// CollaborationPrompt asks the parent to delegate one bounded, tool-free task
// that yields a fresh nonce, then to deliver exactly that nonce. The child is
// told not to run commands: grok-4.6 otherwise reaches for the shell, and a
// command that asks for approval is declined by this headless validator.
const CollaborationPrompt = "Delegate one bounded task to a child named live_child using the default " +
	"full-history fork. Tell the child: Without running any commands or tools, " +
	"write a fresh UUID v4 yourself and reply with exactly its canonical lowercase " +
	"text and no other text. Wait for that child to complete, then reply with " +
	"exactly the UUID returned by the child and no other text."

var (
	// uuidShaped is the nonce contract: eight-four-four-four-twelve hex groups.
	// A model writing a UUID by hand often misses the version and variant
	// nibbles, and those nibbles prove nothing about the Provider binding, so
	// canonical v4 form is a diagnostic rather than an assertion.
	uuidShaped      = regexp.MustCompile(`(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$`)
	canonicalUUIDv4 = regexp.MustCompile(`^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$`)
	codeFence       = regexp.MustCompile("^`{1,3}(?:[a-zA-Z]*\\n)?|`{1,3}$")
)

// nonceOf strips surrounding whitespace and Markdown code fences from a reply
// so that a model quoting the UUID in backticks still yields the bare token.
func nonceOf(reply string) string {
	trimmed := strings.TrimSpace(reply)
	return strings.TrimSpace(codeFence.ReplaceAllString(trimmed, ""))
}

// replyShape is a secret-safe description of a reply for diagnostics.
func replyShape(reply string) string {
	bare := nonceOf(reply)
	switch {
	case bare == "":
		return "empty"
	case canonicalUUIDv4.MatchString(bare):
		return "canonical_uuid_v4"
	case uuidShaped.MatchString(bare):
		return "uuid_shaped"
	case strings.ContainsAny(bare, "\n"):
		return "multi_line"
	case strings.ContainsAny(bare, " \t"):
		return "prose"
	default:
		return "single_token"
	}
}

// Collaboration proves grokex-provider-binding-lifecycle from the session
// graph and the delivered reply:
//
//   - the root Turn completed at Ultra effort under multi-agent v2;
//   - at least one child session names the root as parent and forked from it;
//   - that child completed its own Turn on the Grok provider and model, and
//     its final message is a UUID-shaped nonce;
//   - the root's final message and the reply the app-server delivered both
//     equal that nonce.
func Collaboration(graph *rollout.Graph, run driver.TurnRun, catalog driver.Catalog) Verdict {
	verdict, stage := newVerdict()
	verdict.Assertions["evidence_source"] = EvidenceSource
	verdict.Assertions["runner_turn_submission_count"] = 1
	verdict.Diagnostics["session_file_count"] = len(graph.Sessions)
	verdict.Diagnostics["turn_durations_seconds"] = []float64{seconds(run.Duration)}
	verdict.Diagnostics["notification_kinds"] = run.NotificationKinds
	verdict.Diagnostics["catalog_multi_agent_version"] = catalog.MultiAgentVersion

	root, ok := graph.Session(run.ThreadID)
	if !stage.require("root_session_found", ok && root.ModelProvider == driver.Provider, "root session missing or not bound to grok", "semantic_contract") {
		return *verdict
	}
	verdict.Diagnostics["root_history_mode"] = root.HistoryMode
	verdict.Diagnostics["delivered_turn_status"] = run.Status
	verdict.Diagnostics["final_response_source"] = run.FinalResponseSource
	rootTurn, ok := root.Turn(run.TurnID)
	if ok {
		verdict.Diagnostics["root_turn_state"] = rootTurn.State()
		verdict.Diagnostics["root_function_calls"] = rootTurn.FunctionCallCounts()
		verdict.Diagnostics["root_item_types"] = rootTurn.ItemTypes
		verdict.Diagnostics["root_sub_agent_completions"] = len(rootTurn.SubAgentCompletions)
	}
	if !stage.require("root_turn_completed", ok && rootTurn.Completed && !rootTurn.Failed && run.Completed(), "root Turn did not complete", deadlineOr(run, "semantic_contract")) {
		return *verdict
	}
	verdict.Assertions["parent_completion"] = "completed"
	if !stage.require("root_turn_context_verified", rootTurn.Model == driver.Model && rootTurn.Effort == "ultra" && rootTurn.MultiAgentVersion == "v2", "root Turn context is not grok-4.6 at ultra under multi-agent v2", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["reasoning_effort"] = "ultra"
	verdict.Assertions["multi_agent_version"] = "v2"

	children := graph.Children(root.ID)
	verdict.Diagnostics["child_count"] = len(children)
	var child *rollout.Session
	var childTurn *rollout.Turn
	for _, candidate := range children {
		for _, turn := range candidate.CompletedTurns() {
			if _, inherited := root.Turn(turn.ID); inherited {
				continue
			}
			child, childTurn = candidate, turn
			break
		}
		if child != nil {
			break
		}
	}
	if !stage.require("child_session_linked", child != nil, "no child session names the root as parent and completed a Turn", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["child_parent_link_verified"] = true
	verdict.Assertions["child_completion"] = "completed"
	verdict.Diagnostics["child_history_mode"] = child.HistoryMode
	verdict.Diagnostics["child_function_calls"] = childTurn.FunctionCallCounts()
	if !stage.require("child_forked_from_root", child.ForkedFromID == root.ID, "child did not fork from the root history", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["default_full_history"] = "completed"
	if !stage.require("child_provider_verified", child.ModelProvider == driver.Provider && childTurn.Model == driver.Model, "child is not bound to grok/grok-4.6", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["child_provider_verified"] = true
	verdict.Assertions["child_provider_binding"] = driver.Provider + "/" + driver.Model

	nonce := nonceOf(childTurn.LastAgentMessage)
	verdict.Diagnostics["child_reply_shape"] = replyShape(childTurn.LastAgentMessage)
	verdict.Diagnostics["root_reply_shape"] = replyShape(rootTurn.LastAgentMessage)
	if !stage.require("child_result_nonce", uuidShaped.MatchString(nonce), "child reply is not a UUID-shaped nonce", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["child_response_assertion"] = "uuid_shaped_nonce"
	if !stage.require("root_echo_match", nonceOf(rootTurn.LastAgentMessage) == nonce, "root reply is not the child's nonce", "semantic_contract") {
		return *verdict
	}
	verdict.Assertions["response_assertion"] = "child_echo_match"
	if !stage.require("result_delivered", nonceOf(run.FinalResponse) == nonce, "delivered reply differs from the canonical root reply", "delivery") {
		return *verdict
	}
	verdict.Assertions["result_delivery"] = "completed"
	verdict.Assertions["result_delivery_verified"] = true
	verdict.Assertions["status"] = "completed"
	return *verdict
}

func deadlineOr(run driver.TurnRun, category string) string {
	if run.DeadlineExpired {
		return "deadline"
	}
	return category
}

func seconds(d time.Duration) float64 {
	return float64(d.Milliseconds()) / 1000
}
