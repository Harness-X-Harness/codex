package live

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"regexp"
	"slices"
	"strings"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// ShippedChildCollaboration proves a bounded seed prefix, then asks the shipped
// default model for one delegation task with a default/full-history child. C7 owns real model/tool HTTP composition; this callable
// observer also has deterministic public-protocol proof without backend opt-in.
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
		stream := childStream{diagnostics: server.diagnostics}
		server.setNotification(stream.observe)
		var marker [16]byte
		if _, err := rand.Read(marker[:]); err != nil {
			return errors.New("live: setup marker unavailable")
		}
		setupMarker := hex.EncodeToString(marker[:])
		server.diagnostics.Record("child_setup", map[string]any{"parent_id": parentID, "marker": setupMarker})
		evidence.SetupTurns = 1
		seeded, seed, err := observeShippedTurn(server, parentID, evidence, "", "Remember this bounded setup marker: "+setupMarker+". Reply with a short confirmation.")
		if err != nil {
			return err
		}
		evidence.SetupCompleted, evidence.Completed, evidence.ReplyBytes = true, false, 0
		evidence.Stage, evidence.TaskTurns = "setup_turn_proven", 1
		parent, current, err := observeShippedTurn(server, parentID, evidence, "ultra",
			"Delegate one bounded task to a child named live_child using the default full-history fork. Tell the child: Recall the setup marker from your inherited history and create a fresh UUID v4. Include the marker and your UUID in your answer. Do not ask the parent to supply either value. Wait for that child to complete, then include the UUID returned by the child in your final reply.")
		if err != nil {
			return err
		}
		if current.ID == seed.ID || !retainsSeedPrefix(parent, seeded.Turns) {
			return errors.New("live: parent setup history changed")
		}
		// The parent already knew all conversational setup text, even assistant
		// commentary that the stock child fork does not retain as model input.
		var knownSetup []string
		for _, item := range seed.Items {
			// Observable parent reasoning can reveal a candidate before delegation.
			if reasoning, ok := item.AsReasoning(); ok {
				if reasoning.Content != nil {
					knownSetup = append(knownSetup, (*reasoning.Content)...)
				}
				if reasoning.Summary != nil {
					knownSetup = append(knownSetup, (*reasoning.Summary)...)
				}
			}
			if message, ok := item.AsAgentMessage(); ok {
				knownSetup = append(knownSetup, message.Text)
			}
			if message, ok := item.AsUserMessage(); ok {
				for _, part := range message.Content {
					if text, ok := part.AsText(); ok {
						knownSetup = append(knownSetup, text.Text)
					}
				}
			}
		}
		setupText := strings.Join(knownSetup, "\n")
		parentReply, err := finalProductReply(current)
		if err != nil {
			return err
		}
		// Incidental orchestration is diagnostic. Only a completed, correctly bound
		// child that recalls the inherited setup marker can supply accepted evidence.
		childIDs := []string{}
		childPaths := make(map[string]string)
		parentKnown := make(map[string]string)
		knownBeforeSpawn := []string{setupText}
		for _, item := range current.Items {
			if reasoning, ok := item.AsReasoning(); ok {
				if reasoning.Content != nil {
					knownBeforeSpawn = append(knownBeforeSpawn, (*reasoning.Content)...)
				}
				if reasoning.Summary != nil {
					knownBeforeSpawn = append(knownBeforeSpawn, (*reasoning.Summary)...)
				}
			}
			if message, ok := item.AsAgentMessage(); ok {
				knownBeforeSpawn = append(knownBeforeSpawn, message.Text)
			}
			if message, ok := item.AsUserMessage(); ok {
				for _, part := range message.Content {
					if text, ok := part.AsText(); ok {
						knownBeforeSpawn = append(knownBeforeSpawn, text.Text)
					}
				}
			}
			activity, ok := item.AsSubAgentActivity()
			if !ok || activity.Kind != "started" {
				continue
			}
			id, path := activity.AgentThreadID, activity.AgentPath
			if id == "" || id == parentID || path == "" || len(path) > 1024 {
				server.diagnostics.Record("child_candidate_skipped", map[string]any{"child_id": id, "path": path, "reason": "invalid_activity_identity"})
				continue
			}
			if previous, seen := childPaths[id]; seen {
				if previous != path {
					server.diagnostics.Record("child_candidate_skipped", map[string]any{"child_id": id, "path": path, "previous_path": previous, "reason": "conflicting_activity"})
					return errors.New("live: conflicting child activity")
				}
				continue
			}
			childIDs = append(childIDs, id)
			childPaths[id] = path
			// Public items are ordered by creation ordinal; Started is emitted
			// after spawn. A pre-spawn parent result may enter hidden model context.
			parentKnown[id] = strings.Join(knownBeforeSpawn, "\n")
			if len(childIDs) > 16 {
				return errors.New("live: child evidence budget exceeded")
			}
		}
		children := []productThread{}
		for _, childID := range childIDs {
			child, bindingErr := readProductThread(server, childID, evidence.Model)
			eligible := bindingErr == nil && child.ParentThreadID == parent.ID && child.ForkedFromID == parent.ID && child.Source.SubAgent.ThreadSpawn.AgentPath == childPaths[childID]
			server.diagnostics.Record("child_candidate", map[string]any{"child_id": childID, "observed_path": childPaths[childID], "parent_id": child.ParentThreadID, "forked_from_id": child.ForkedFromID, "path": child.Source.SubAgent.ThreadSpawn.AgentPath, "eligible": eligible, "read_failed": bindingErr != nil})
			if !eligible {
				continue
			}
			children = append(children, child)
		}
		if err := stream.failure(); err != nil {
			return err
		}
		var credited []func() bool
		for waited := 0; ; waited++ {
			matched, pending := false, false
			for _, child := range children {
				for _, turn := range child.Turns {
					inheritedTurn := false
					for _, inherited := range parent.Turns {
						if inherited.ID == turn.ID {
							inheritedTurn = true
						}
					}
					if inheritedTurn || completedProductTurn(turn) != nil {
						server.diagnostics.Record("child_turn_skipped", map[string]any{"child_id": child.ID, "turn_id": turn.ID, "inherited": inheritedTurn, "status": turn.Status})
						if !inheritedTurn {
							recordProductDiagnosticTurn(server.diagnostics, "child_incomplete_result", child.ID, turn)
						}
						continue
					}
					recordProductDiagnosticTurn(server.diagnostics, "child_result", child.ID, turn)
					reply, replyErr := finalProductReply(turn)
					unique := make(map[string]bool)
					for _, value := range childNonce.FindAllString(strings.ToLower(reply), -1) {
						unique[value] = true
					}
					server.diagnostics.Record("child_reply_check", map[string]any{"child_id": child.ID, "turn_id": turn.ID, "reply_available": replyErr == nil, "uuid_count": len(unique), "marker_recalled": containsProofValue(reply, setupMarker)})
					if replyErr != nil || len(unique) == 0 || !containsProofValue(reply, setupMarker) {
						continue
					}
					// Credit a fresh child result actually used by the parent. Other
					// UUIDs and surrounding prose do not invalidate that result.
					for nonce := range unique {
						server.diagnostics.Record("child_result_use", map[string]any{"child_id": child.ID, "turn_id": turn.ID, "uuid": nonce, "used_by_parent": containsProofValue(parentReply, nonce), "known_before_spawn": containsProofValue(parentKnown[child.ID], nonce)})
						if !containsProofValue(parentReply, nonce) || containsProofValue(parentKnown[child.ID], nonce) {
							continue
						}
						proof := stream.proof(child, parent, turn, nonce, setupMarker)
						server.diagnostics.Record("child_stream_check", map[string]any{"child_id": child.ID, "turn_id": turn.ID, "proof": proof})
						switch proof {
						case childNotProven:
							continue
						case childPending:
							pending = true
							continue
						case childProven:
						}
						evidence.ChildBound, evidence.ChildCompleted = true, true
						credited = append(credited, func() bool {
							return stream.proof(child, parent, turn, nonce, setupMarker) == childProven
						})
						matched = true
					}
				}
			}
			if matched {
				break
			}
			if !pending {
				return failedObservation("child_result_not_proven", "live: qualifying child result absent")
			}
			if waited == childStreamEventLimit {
				return errors.New("live: child stream budget exceeded")
			}
			// History and listener notifications travel independently. Wait only for
			// already-generated evidence within the original process deadline; do not
			// submit another operation or observe pending notifications twice.
			message, err := server.next()
			if err != nil {
				return err
			}
			for _, id := range []string{seed.ID, current.ID} {
				if _, err := productTerminal(message, parentID, id); err != nil {
					return err
				}
			}
		}
		server.finalize = func() error {
			if err := stream.failure(); err != nil {
				return err
			}
			for _, message := range stream.snapshot() {
				for _, id := range []string{seed.ID, current.ID} {
					if _, err := productTerminal(message, parentID, id); err != nil {
						return err
					}
				}
			}
			// Only candidates already credited before cleanup are eligible.
			for _, valid := range credited {
				if valid() {
					return nil
				}
			}
			return failedObservation("child_result_not_proven", "live: qualifying child result absent")
		}
		if err := server.finalize(); err != nil {
			return err
		}
		evidence.ChildResultDelivered, evidence.Stage = true, "child_result_delivered"
		return nil
	})
}

func retainsSeedPrefix(thread productThread, seed []productTurn) bool {
	if len(seed) == 0 || len(thread.Turns) < len(seed) {
		return false
	}
	// The text setup preserves role/content/order and successful turn identity,
	// not reconstructed item IDs or diagnostic metadata.
	type textItem struct{ role, text string }
	conversation := func(turn productTurn) ([]textItem, bool) {
		var result []textItem
		for _, item := range turn.Items {
			if message, ok := item.AsUserMessage(); ok {
				var parts []string
				for _, part := range message.Content {
					text, ok := part.AsText()
					if !ok {
						return nil, false
					}
					parts = append(parts, text.Text)
				}
				result = append(result, textItem{"user", strings.Join(parts, "\n")})
			}
			if message, ok := item.AsAgentMessage(); ok {
				result = append(result, textItem{"assistant", message.Text})
			}
		}
		return result, true
	}
	for index, original := range seed {
		inherited := thread.Turns[index]
		if inherited.ID != original.ID || inherited.Status != original.Status || completedProductTurn(inherited) != nil {
			return false
		}
		before, beforeOK := conversation(original)
		after, afterOK := conversation(inherited)
		if !beforeOK || !afterOK || !slices.Equal(before, after) {
			return false
		}
	}
	return true
}

// The proof values are hexadecimal: ordinary letter casing does not make a
// supplied value fresh or an accurately recalled marker different.
func containsProofValue(text, value string) bool {
	return strings.Contains(strings.ToLower(text), value)
}

var childNonce = regexp.MustCompile(`[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}`)

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
	if thread.ID != id || thread.Model != model || thread.Provider != "grok" || len(thread.Turns) > 32 {
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

func observeProductInputTurn(server *sdkProductObserver, id string, evidence *Evidence, effort string, input []protocolv2.UserInput) (productThread, productTurn, error) {
	thread, terminal, err := observeProductCompletedTurn(server, id, evidence, effort, input)
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
