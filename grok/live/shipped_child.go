package live

import (
	"context"
	"encoding/json"
	"errors"
	"strings"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// ShippedChildCollaboration asks for a useful delegated invoice. Authenticity
// comes from runtime child binding, its durable result, consumed delivery and
// the subsequent parent result, not model-facing identifiers or nonce recall.
func ShippedChildCollaboration(ctx context.Context, subject Subject, key string, sinks ...*Diagnostics) (Evidence, error) {
	options, err := shippedOptions(subject, key, "")
	if err != nil {
		return Evidence{Stage: "preflight"}, err
	}
	if len(sinks) != 0 {
		options.Diagnostics = sinks[0]
	}
	return runShippedChildCollaboration(ctx, options)
}

func runShippedChildCollaboration(ctx context.Context, options Options) (Evidence, error) {
	return sdkShippedSession(ctx, options, func(server *sdkProductObserver, parentID string, evidence *Evidence) error {
		stream := childStream{diagnostics: server.diagnostics, parentID: parentID}
		server.setNotification(stream.observe)
		evidence.TaskTurns = 1
		parent, current, err := observeProductCompletedTurn(server, parentID, evidence, "", []protocolv2.UserInput{
			protocolv2.NewUserInputText(protocolv2.UserInputText{Text: childInvoiceTask}),
		})
		if err != nil {
			return err
		}
		candidates, err := childCandidates(current, parentID, stream.snapshot())
		if err != nil {
			return err
		}
		var children []productThread
		bound := make(map[string]bool)
		paths := make(map[string]string)
		for id, path := range candidates {
			child, readErr := readProductThread(server, id, "")
			childPath := child.Source.SubAgent.ThreadSpawn.AgentPath
			eligible := readErr == nil && child.ParentThreadID == parentID && childPath != "" && childPath != "/root" && len(childPath) <= 1024 &&
				(child.Model == providerfixture.PrimaryModel || child.Model == providerfixture.PinnedModel) && (path == "" || path == childPath)
			server.diagnostics.Record("child_candidate", map[string]any{"child_id": id, "parent_id": child.ParentThreadID, "forked_from_id": child.ForkedFromID, "path": childPath, "observed_path": path, "eligible": eligible, "read_failed": readErr != nil})
			if !eligible {
				continue
			}
			if prior := paths[childPath]; prior != "" && prior != id {
				return failedObservation("child_binding_ambiguous", "live: child binding ambiguous")
			}
			paths[childPath], bound[id], evidence.ChildBound = id, true, true
			children = append(children, child)
		}
		parentReply, parentErr := childParentTerminalReply(current, bound)
		if parentErr == nil {
			evidence.Completed, evidence.ReplyBytes, evidence.Stage = true, len(parentReply), "product_turn_proven"
		}
		parentValid := parentErr == nil && validChildInvoice(parentReply)
		server.diagnostics.Record("child_parent_result", map[string]any{"reply": parentReply, "reply_available": parentErr == nil, "invoice_correct": parentValid})
		type candidate struct {
			child productThread
			turn  productTurn
		}
		var qualifying, completed []candidate
		for _, child := range children {
			for _, turn := range child.Turns {
				inherited := false
				for _, prior := range parent.Turns {
					inherited = inherited || prior.ID == turn.ID
				}
				if inherited {
					continue
				}
				recordProductDiagnosticTurn(server.diagnostics, "child_result", child.ID, turn)
				if completedProductTurn(turn) != nil {
					continue
				}
				evidence.ChildCompleted = true
				completed = append(completed, candidate{child, turn})
				reply, replyErr := childParentTerminalReply(turn, nil)
				correct := replyErr == nil && validChildInvoice(reply)
				server.diagnostics.Record("child_reply_check", map[string]any{"child_id": child.ID, "turn_id": turn.ID, "reply_available": replyErr == nil, "invoice_correct": correct})
				if correct {
					qualifying = append(qualifying, candidate{child, turn})
				}
			}
		}
		// Preserve independently verified child completion on business failure,
		// but recheck admitted terminal contradictions when cleanup joins callbacks.
		if len(completed) != 0 {
			server.finalize = func() error {
				if err := stream.failure(); err != nil {
					return err
				}
				for _, candidate := range completed {
					if !stream.terminalConflict(candidate.child.ID, candidate.turn.ID) {
						return nil
					}
				}
				return failedObservation("child_terminal_contradiction", "live: child completion contradicted")
			}
		}
		if !evidence.ChildBound {
			return failedObservation("child_binding_not_observed", "live: bound child not observed")
		}
		if len(qualifying) == 0 {
			return failedObservation("child_invoice_not_observed", "live: completed child invoice not observed")
		}
		if !parentValid {
			return failedObservation("child_parent_result_invalid", "live: parent invoice invalid or absent")
		}
		var credited []candidate
		for {
			if err := stream.failure(); err != nil {
				return err
			}
			pending := false
			for _, candidate := range qualifying {
				proof := stream.proof(candidate.child, parent, candidate.turn, current, bound)
				server.diagnostics.Record("child_stream_check", map[string]any{"child_id": candidate.child.ID, "turn_id": candidate.turn.ID, "proof": proof})
				if proof == childProven {
					credited = append(credited, candidate)
				}
				pending = pending || proof == childPending
			}
			if len(credited) > 0 {
				break
			}
			if !pending {
				return failedObservation("child_delivery_not_observed", "live: child result delivery not observed")
			}
			// Durable reads and callbacks are separate channels. Wait within the
			// original deadline for already-generated evidence, never another task.
			message, err := server.next()
			if err != nil {
				return errors.Join(failedObservation("child_delivery_not_observed", "live: child result delivery not observed"), err)
			}
			if _, err := productTerminal(message, parentID, current.ID); err != nil {
				return err
			}
		}
		server.finalize = func() error {
			if err := stream.failure(); err != nil {
				return err
			}
			for _, candidate := range credited {
				if stream.proof(candidate.child, parent, candidate.turn, current, bound) == childProven {
					return nil
				}
			}
			return failedObservation("child_delivery_not_observed", "live: child result delivery not observed")
		}
		if err := server.finalize(); err != nil {
			return err
		}
		evidence.ChildResultDelivered, evidence.Stage = true, "child_result_delivered"
		return nil
	})
}

// Discovery hints are not binding proof. Started is emitted only after stock
// creation succeeds, so completed activity, collab receivers and child terminal
// hints can also lead to an authoritative thread/read.
func childCandidates(turn productTurn, parentID string, events []frame) (map[string]string, error) {
	candidates := make(map[string]string)
	add := func(id, path string) error {
		if id == "" || id == parentID || len(path) > 1024 {
			return nil
		}
		if old, seen := candidates[id]; seen && old != "" && path != "" && old != path {
			return failedObservation("child_binding_ambiguous", "live: conflicting child activity")
		}
		if candidates[id] == "" || path != "" {
			candidates[id] = path
		}
		if len(candidates) > 16 {
			return failedObservation("child_candidate_evidence_budget_exceeded", "live: child candidate evidence budget exceeded")
		}
		return nil
	}
	for _, item := range turn.Items {
		if activity, ok := item.AsSubAgentActivity(); ok {
			if err := add(activity.AgentThreadID, activity.AgentPath); err != nil {
				return nil, err
			}
		}
		if call, ok := item.AsCollabAgentToolCall(); ok && call.SenderThreadID == parentID {
			for _, id := range call.ReceiverThreadIDs {
				if err := add(id, ""); err != nil {
					return nil, err
				}
			}
		}
	}
	for _, event := range events {
		if value, ok := event.Notification.AsTurnCompleted(); ok {
			if err := add(value.Params.ThreadID, ""); err != nil {
				return nil, err
			}
		}
	}
	return candidates, nil
}

type productTurn struct {
	ID     string                  `json:"id"`
	Status string                  `json:"status"`
	Error  json.RawMessage         `json:"error"`
	Items  []protocolv2.ThreadItem `json:"items"`
}
type productThread struct {
	ID             string              `json:"id"`
	Model          string              `json:"model"`
	Provider       string              `json:"modelProvider"`
	ParentThreadID string              `json:"parentThreadId"`
	ForkedFromID   string              `json:"forkedFromId"`
	Turns          []productTurn       `json:"turns"`
	Source         productThreadSource `json:"source"`
}

// SessionSource is a public union: root origins are strings; spawned child
// provenance is an object. Keep child ownership fields separate from root forms.
type productThreadSource struct {
	SubAgent struct {
		ThreadSpawn struct {
			AgentPath string `json:"agent_path"`
		} `json:"thread_spawn"`
	} `json:"subAgent"`
}

func completedProductTurn(turn productTurn) error {
	if turn.ID == "" {
		return failedObservation("turn_identity_missing", "live: bound turn did not complete")
	}
	if turn.Status != "completed" || (len(turn.Error) != 0 && string(turn.Error) != "null") {
		return failedTurn(turn.Status, turn.Error, "live: bound turn did not complete")
	}
	return nil
}

// The parent can emit phase-less status messages in sampling rounds that
// continue through tools. Rust session/turn.rs assigns its terminal answer only
// when needs_follow_up is false; public history preserves tool creation order.
// Select that final tool-free segment, not the last parseable invoice or all rounds.
// This is deliberately local to the delegated invoice consumer.
func childParentTerminalReply(turn productTurn, boundChildren map[string]bool) (string, error) {
	items, err := childTerminalItems(turn, boundChildren)
	if err != nil {
		return "", err
	}
	return finalProductReply(productTurn{Items: items})
}

func childTerminalItems(turn productTurn, boundChildren map[string]bool) ([]protocolv2.ThreadItem, error) {
	if len(turn.Items) > 128 {
		return nil, errors.New("live: product item budget exceeded")
	}
	identities := make(map[string][2]string)
	var messages []protocolv2.ThreadItem
	terminalStart := 0
	for _, value := range turn.Items {
		status := ""
		switch value.Kind() {
		case protocolv2.ThreadItemKindCommandExecution:
			item, _ := value.AsCommandExecution()
			status = string(item.Status)
		case protocolv2.ThreadItemKindFileChange:
			item, _ := value.AsFileChange()
			status = string(item.Status)
		case protocolv2.ThreadItemKindMCPToolCall:
			item, _ := value.AsMCPToolCall()
			status = string(item.Status)
		case protocolv2.ThreadItemKindDynamicToolCall:
			item, _ := value.AsDynamicToolCall()
			status = string(item.Status)
		case protocolv2.ThreadItemKindCollabAgentToolCall:
			item, _ := value.AsCollabAgentToolCall()
			status = string(item.Status)
		case protocolv2.ThreadItemKindFunctionCallOutput:
			status = "completed"
		case protocolv2.ThreadItemKindSubAgentActivity:
			item, _ := value.AsSubAgentActivity()
			// multi_agents_v2/spawn.rs emits Started with the tool call ID only
			// after successful spawn. Other lifecycle activity is asynchronous.
			// The accepted result still requires the actual lineage/read binding.
			if item.Kind == "started" && item.ID != "" && boundChildren[item.AgentThreadID] && item.AgentPath != "" && len(item.AgentPath) <= 1024 {
				status = "completed"
			}
		}
		// Failed/declined tools can have legitimate subsequent recovery. A
		// pending activity is not evidence of a finished tool round.
		if status == "completed" || status == "failed" || status == "declined" {
			terminalStart = len(messages)
		}
		// Completed/other child lifecycle activity must never erase a final
		// reply or hide invalid suffixes.
		item, ok := value.AsAgentMessage()
		if !ok {
			continue
		}
		phase := ""
		if item.Phase != nil && item.Phase.Value != nil {
			phase = string(*item.Phase.Value)
		}
		if item.ID == "" {
			return nil, errors.New("live: invalid product reply")
		}
		identity := [2]string{phase, strings.TrimSpace(item.Text)}
		if previous, seen := identities[item.ID]; seen {
			if previous != identity {
				return nil, errors.New("live: conflicting product reply")
			}
			continue
		}
		identities[item.ID] = identity
		messages = append(messages, value)
	}
	// Only the terminal segment consumes the 4096-byte result budget. Full
	// bounded public history stays available for diagnostics and freshness.
	return messages[terminalStart:], nil
}

func finalProductReply(turn productTurn) (string, error) {
	if len(turn.Items) > 128 {
		return "", errors.New("live: product item budget exceeded")
	}
	var final, legacy []string
	identities := make(map[string][2]string)
	size := 0
	for _, value := range turn.Items {
		item, ok := value.AsAgentMessage()
		if !ok {
			continue
		}
		phase := ""
		if item.Phase != nil && item.Phase.Value != nil {
			phase = string(*item.Phase.Value)
		}
		if phase != "" && phase != "final_answer" {
			continue
		}
		text := strings.TrimSpace(item.Text)
		if item.ID == "" {
			return "", errors.New("live: invalid product reply")
		}
		value := [2]string{phase, text}
		if previous, seen := identities[item.ID]; seen {
			if previous != value {
				return "", errors.New("live: conflicting product reply")
			}
			continue
		}
		identities[item.ID] = value
		// Stock finalization can retain a message after stripping hidden markup.
		// It contributes no answer, but its identity must still detect conflicts.
		if text == "" {
			continue
		}
		size += len(text)
		if size > 4096 {
			return "", errors.New("live: product reply budget exceeded")
		}
		if phase == "final_answer" {
			final = append(final, text)
		} else {
			legacy = append(legacy, text)
		}
	}
	if len(final) == 0 {
		final = legacy
	}
	if len(final) == 0 {
		return "", errors.New("live: completed product reply absent")
	}
	return strings.Join(final, "\n"), nil
}

func readProductThread(server *sdkProductObserver, id, model string) (productThread, error) {
	include := true
	response, err := server.client.Threads().Read(server.ctx, protocolv2.ThreadReadParams{ThreadID: id, IncludeTurns: &include})
	if err != nil {
		return productThread{}, shippedTransport(server.ctx, err)
	}
	server.diagnostics.Record("product_read_snapshot", map[string]any{"thread_id": response.Thread.ID, "expected_thread_id": id, "model": response.Thread.Model, "provider": response.Thread.ModelProvider, "turn_count": len(response.Thread.Turns), "omitted_turns": max(0, len(response.Thread.Turns)-32)})
	for _, value := range response.Thread.Turns[:min(32, len(response.Thread.Turns))] {
		recordSDKDiagnosticTurn(server.diagnostics, "product_read_turn", response.Thread.ID, value)
	}
	thread, err := productThreadFromSDK(response.Thread)
	if err != nil {
		return thread, errors.New("live: invalid product completion")
	}
	if thread.ID != id || (model != "" && thread.Model != model) || thread.Provider != "grok" || len(thread.Turns) > 32 {
		return thread, errors.New("live: durable product binding mismatch")
	}
	return thread, nil
}

func productTerminal(message frame, threadID, turnID string) (bool, error) {
	if message.Method != "turn/completed" {
		return false, nil
	}
	value, ok := message.Notification.AsTurnCompleted()
	if !ok {
		return false, errors.New("live: invalid product completion")
	}
	if value.Params.ThreadID != threadID || value.Params.Turn.ID != turnID {
		return false, nil
	}
	turn, err := productTurnFromSDK(value.Params.Turn)
	if err != nil {
		return false, errors.New("live: invalid product completion")
	}
	return true, completedProductTurn(turn)
}

func observeShippedTurn(server *sdkProductObserver, id string, evidence *Evidence, effort, prompt string) (productThread, productTurn, error) {
	return observeProductInputTurn(server, id, evidence, effort, []protocolv2.UserInput{
		protocolv2.NewUserInputText(protocolv2.UserInputText{Text: prompt}),
	})
}

func observeProductInputTurn(server *sdkProductObserver, id string, evidence *Evidence, effort string, input []protocolv2.UserInput, outputSchema ...*protocolv2.OutputSchema) (productThread, productTurn, error) {
	thread, terminal, err := observeProductCompletedTurn(server, id, evidence, effort, input, outputSchema...)
	if err != nil {
		return thread, terminal, err
	}
	text, err := finalProductReply(terminal)
	if err != nil {
		return thread, terminal, err
	}
	evidence.Completed, evidence.ReplyBytes, evidence.Stage = true, len(text), "product_turn_proven"
	return thread, terminal, nil
}

// A completed image result need not also contain an assistant text message.
func observeProductCompletedTurn(server *sdkProductObserver, id string, evidence *Evidence, effort string, input []protocolv2.UserInput, outputSchema ...*protocolv2.OutputSchema) (productThread, productTurn, error) {
	var texts []string
	for _, part := range input {
		if text, ok := part.AsText(); ok {
			texts = append(texts, text.Text)
		}
	}
	var schema *protocolv2.OutputSchema
	if len(outputSchema) > 0 {
		schema = outputSchema[0]
	}
	server.diagnostics.Record("product_turn_request", map[string]any{"thread_id": id, "effort": effort, "text": texts, "input_count": len(input), "output_schema": schema})
	evidence.Turns++
	request := protocolv2.TurnStartParams{ThreadID: id, Input: input, OutputSchema: schema}
	if effort != "" {
		request.Effort = protocolv2.Value(protocolv2.ReasoningEffort(effort))
	}
	response, err := server.client.Turns().Start(server.ctx, request)
	if err != nil {
		return productThread{}, productTurn{}, shippedTransport(server.ctx, err)
	}
	recordSDKDiagnosticTurn(server.diagnostics, "product_inline_snapshot", id, response.Turn)
	terminal, err := productTurnFromSDK(response.Turn)
	if err != nil {
		return productThread{}, terminal, errors.New("live: invalid product completion")
	}
	recordProductDiagnosticTurn(server.diagnostics, "product_turn_start", id, terminal)
	if terminal.ID == "" {
		return productThread{}, terminal, errors.New("live: product turn identity absent")
	}
	evidence.Stage = "turn_submitted"

	if terminal.Status == "inProgress" {
		for {
			message, err := server.next()
			if err != nil {
				return productThread{}, terminal, err
			}
			complete, err := productTerminal(message, id, terminal.ID)
			if err != nil {
				return productThread{}, terminal, err
			}
			if complete {
				break
			}
		}
	} else if err := completedProductTurn(terminal); err != nil {
		return productThread{}, terminal, err
	}
	thread, err := readProductThread(server, id, evidence.Model)
	if err != nil {
		return thread, terminal, err
	}
	for _, message := range server.queued() {
		if _, err := productTerminal(message, id, terminal.ID); err != nil {
			return thread, terminal, err
		}
	}
	found := 0
	for _, turn := range thread.Turns {
		if turn.ID == terminal.ID {
			recordProductDiagnosticTurn(server.diagnostics, "product_turn_settled", id, turn)
			terminal = turn
			found++
		}
	}
	if found != 1 {
		return thread, terminal, errors.New("live: bound durable turn absent or duplicated")
	}
	if err := completedProductTurn(terminal); err != nil {
		return thread, terminal, err
	}
	return thread, terminal, nil
}

// Preserve public task output before validating it; never serialize raw history or reasoning.
func recordProductDiagnosticTurn(diagnostics *Diagnostics, source, threadID string, turn productTurn) {
	if diagnostics == nil {
		return
	}
	fields := map[string]any{"thread_id": threadID, "turn_id": turn.ID, "status": turn.Status}
	if len(turn.Error) != 0 {
		fields["error"] = turn.Error
	}
	if turn.Status == "failed" || turn.Status == "interrupted" {
		if err := completedProductTurn(turn); err != nil {
			fields["failure"] = DescribeFailure(err)
		}
	}
	diagnostics.Record(source, fields)
	for _, item := range turn.Items {
		if _, reasoning := item.AsReasoning(); reasoning {
			continue
		}
		raw, err := json.Marshal(item)
		if err == nil {
			diagnostics.Item(source, raw)
		}
	}
}

// Observe decoded public snapshots before oracle validation can reject them.
func recordSDKDiagnosticTurn(diagnostics *Diagnostics, source, threadID string, value protocolv2.Turn) {
	if diagnostics == nil {
		return
	}
	turn := productTurn{ID: value.ID, Status: string(value.Status), Items: value.Items}
	if value.Error != nil && value.Error.Value != nil {
		turn.Error, _ = json.Marshal(*value.Error.Value)
	}
	recordProductDiagnosticTurn(diagnostics, source, threadID, turn)
}
