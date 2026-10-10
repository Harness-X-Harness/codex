package live

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"reflect"
	"regexp"
	"strings"
)

// ShippedChildCollaboration proves a bounded seed prefix, then asks the shipped
// default model for one delegation task with a default/full-history child. C7 owns real model/tool HTTP composition; this callable
// observer also has deterministic public-protocol proof without backend opt-in.
func ShippedChildCollaboration(ctx context.Context, subject Subject, key string) (Evidence, error) {
	options, err := shippedOptions(subject, key, "")
	if err != nil {
		return Evidence{Stage: "preflight"}, err
	}
	return runShippedChildCollaboration(ctx, options)
}

func runShippedChildCollaboration(ctx context.Context, options Options) (Evidence, error) {
	return runFixture(ctx, options, nil, map[string]any{"experimentalRawEvents": true}, func(server *appServer, parentID string, evidence *Evidence) error {
		stream := childStream{}
		server.notification = stream.observe
		var marker [16]byte
		if _, err := rand.Read(marker[:]); err != nil {
			return errors.New("live: setup marker unavailable")
		}
		setupMarker := hex.EncodeToString(marker[:])
		evidence.SetupTurns = 1
		seeded, seed, err := observeShippedTurn(server, parentID, evidence, "", "Remember this bounded setup marker: "+setupMarker+". Reply with a short confirmation without using tools.")
		if err != nil {
			return err
		}
		evidence.SetupCompleted, evidence.Completed, evidence.ReplyBytes = true, false, 0
		evidence.Stage, evidence.TaskTurns = "setup_turn_proven", 1
		parent, current, err := observeShippedTurn(server, parentID, evidence, "ultra",
			"Delegate one bounded task to a child named live_child using the default full-history fork. Tell the child: Without running commands or tools, recall the setup marker from your inherited history and write a fresh UUID v4 yourself. Reply with the setup marker and the canonical lowercase UUID. Do not ask the parent to supply the marker. Wait for that child to complete, then include the UUID returned by the child in your final reply.")
		if err != nil {
			return err
		}
		if current.ID == seed.ID || !retainsSeedPrefix(parent, seeded.Turns) {
			return errors.New("live: parent setup history changed")
		}
		// The parent already knew all conversational setup text, even assistant
		// commentary that the stock child fork does not retain as model input.
		var knownSetup []string
		for _, raw := range seed.Items {
			var item struct {
				Type, Text string
				Content    []struct{ Type, Text string }
			}
			if json.Unmarshal(raw, &item) != nil {
				return errors.New("live: invalid setup item")
			}
			switch item.Type {
			case "agentMessage":
				knownSetup = append(knownSetup, item.Text)
			case "userMessage":
				for _, part := range item.Content {
					if part.Type == "text" {
						knownSetup = append(knownSetup, part.Text)
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
		for _, raw := range current.Items {
			var activity struct {
				Type, Kind, AgentThreadID, AgentPath, Text string
				Content                                    []struct{ Type, Text string }
			}
			if json.Unmarshal(raw, &activity) != nil {
				return errors.New("live: invalid child activity")
			}
			switch activity.Type {
			case "agentMessage":
				knownBeforeSpawn = append(knownBeforeSpawn, activity.Text)
			case "userMessage":
				for _, part := range activity.Content {
					if part.Type == "text" {
						knownBeforeSpawn = append(knownBeforeSpawn, part.Text)
					}
				}
			}
			if activity.Type != "subAgentActivity" || activity.Kind != "started" {
				continue
			}
			id, path := activity.AgentThreadID, activity.AgentPath
			if id == "" || id == parentID || path == "" || len(path) > 1024 {
				continue
			}
			if previous, seen := childPaths[id]; seen {
				if previous != path {
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
			if !eligible {
				continue
			}
			children = append(children, child)
		}
		// All child reads finish before selecting proof, so conflicts queued during
		// any RPC can invalidate the exact credited child without poisoning others.
		if stream.err != nil {
			return stream.err
		}
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
						continue
					}
					reply, replyErr := finalProductReply(turn)
					unique := make(map[string]bool)
					for _, value := range childNonce.FindAllString(reply, -1) {
						unique[value] = true
					}
					if replyErr != nil || len(unique) != 1 || !containsProofValue(reply, setupMarker) {
						continue
					}
					var nonce string
					for value := range unique {
						nonce = value
					}
					if !containsProofValue(parentReply, nonce) || containsProofValue(parentKnown[child.ID], nonce) {
						continue
					}
					switch stream.proof(child, parent, turn, reply, nonce, setupMarker) {
					case childNotProven:
						continue
					case childPending:
						pending = true
						continue
					case childProven:
					}
					evidence.ChildBound, evidence.ChildCompleted = true, true
					matched = true
				}
			}
			if matched {
				break
			}
			if !pending {
				return errors.New("live: qualifying child result absent")
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
			if len(message.ID) != 0 {
				return server.refuse(message.ID)
			}
			for _, id := range []string{seed.ID, current.ID} {
				if _, err := productTerminal(message, parentID, id); err != nil {
					return err
				}
			}
		}
		for _, message := range stream.events {
			for _, id := range []string{seed.ID, current.ID} {
				if _, err := productTerminal(message, parentID, id); err != nil {
					return err
				}
			}
		}
		evidence.ChildResultDelivered, evidence.Stage = true, "child_result_delivered"
		return nil
	})
}

func retainsSeedPrefix(thread productThread, seed []productTurn) bool {
	if len(seed) == 0 || len(thread.Turns) < len(seed) {
		return false
	}
	for index, original := range seed {
		inherited := thread.Turns[index]
		if inherited.ID != original.ID || inherited.Status != original.Status || completedProductTurn(inherited) != nil || len(inherited.Items) != len(original.Items) {
			return false
		}
		for i, item := range original.Items {
			var before, after any
			source := json.NewDecoder(bytes.NewReader(item))
			source.UseNumber()
			copied := json.NewDecoder(bytes.NewReader(inherited.Items[i]))
			copied.UseNumber()
			if source.Decode(&before) != nil || copied.Decode(&after) != nil || !reflect.DeepEqual(before, after) {
				return false
			}
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
	ID     string            `json:"id"`
	Status string            `json:"status"`
	Error  json.RawMessage   `json:"error"`
	Items  []json.RawMessage `json:"items"`
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

func (source *productThreadSource) UnmarshalJSON(data []byte) error {
	data = bytes.TrimSpace(data)
	if len(data) != 0 && data[0] == '"' {
		var origin string
		if err := json.Unmarshal(data, &origin); err != nil {
			return err
		}
		switch origin {
		case "cli", "vscode", "exec", "appServer", "unknown":
			*source = productThreadSource{}
			return nil
		default:
			return errors.New("live: unknown product thread source")
		}
	}
	type objectSource productThreadSource
	var decoded objectSource
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	*source = productThreadSource(decoded)
	return nil
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
	for _, raw := range turn.Items {
		var item struct{ ID, Type, Phase, Text string }
		if json.Unmarshal(raw, &item) != nil {
			return "", errors.New("live: invalid product item")
		}
		if item.Type != "agentMessage" || (item.Phase != "" && item.Phase != "final_answer") {
			continue
		}
		text := strings.TrimSpace(item.Text)
		if item.ID == "" || text == "" {
			return "", errors.New("live: invalid product reply")
		}
		value := [2]string{item.Phase, text}
		if previous, seen := identities[item.ID]; seen {
			if previous != value {
				return "", errors.New("live: conflicting product reply")
			}
			continue
		}
		identities[item.ID] = value
		size += len(text)
		if size > 4096 {
			return "", errors.New("live: product reply budget exceeded")
		}
		if item.Phase == "final_answer" {
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

func readProductThread(server *appServer, id, model string) (productThread, error) {
	var response struct {
		Thread productThread `json:"thread"`
	}
	err := server.call("thread/read", map[string]any{"threadId": id, "includeTurns": true}, &response)
	if err != nil {
		return response.Thread, err
	}
	thread := response.Thread
	if thread.ID != id || thread.Model != model || thread.Provider != "grok" || len(thread.Turns) > 32 {
		return thread, errors.New("live: durable product binding mismatch")
	}
	return thread, nil
}

func productTerminal(message frame, threadID, turnID string) (bool, error) {
	if message.Method != "turn/completed" {
		return false, nil
	}
	var event struct {
		ThreadID string      `json:"threadId"`
		Turn     productTurn `json:"turn"`
	}
	if json.Unmarshal(message.Params, &event) != nil {
		return false, errors.New("live: invalid product completion")
	}
	if event.ThreadID != threadID || event.Turn.ID != turnID {
		return false, nil
	}
	return true, completedProductTurn(event.Turn)
}

func observeShippedTurn(server *appServer, id string, evidence *Evidence, effort, prompt string) (productThread, productTurn, error) {
	evidence.Turns++
	var response struct {
		Turn productTurn `json:"turn"`
	}
	params := map[string]any{"threadId": id, "input": []any{map[string]any{"type": "text", "text": prompt, "textElements": []any{}}}}
	if effort != "" {
		params["effort"] = effort
	}
	if err := server.call("turn/start", params, &response); err != nil {
		return productThread{}, productTurn{}, err
	}
	terminal := response.Turn
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
			if len(message.ID) != 0 {
				return productThread{}, terminal, server.refuse(message.ID)
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
	for _, message := range server.pending {
		if _, err := productTerminal(message, id, terminal.ID); err != nil {
			return thread, terminal, err
		}
	}
	found := 0
	for _, turn := range thread.Turns {
		if turn.ID == terminal.ID {
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
	text, err := finalProductReply(terminal)
	if err != nil {
		return thread, terminal, err
	}
	evidence.Completed, evidence.ReplyBytes, evidence.Stage = true, len(text), "product_turn_proven"
	return thread, terminal, nil
}
