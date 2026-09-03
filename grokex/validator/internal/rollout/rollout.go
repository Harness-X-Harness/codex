// Package rollout reads the session rollouts a Codex home persists under
// sessions/ and rebuilds the causal graph a Live Story is proven from: which
// sessions exist, which one spawned which, and what each Turn completed with.
//
// Rollout lines are Server Observations: known members are read exactly and
// unknown members are ignored, so upstream additions do not break the reader.
// Raw lines never leave this package; callers receive structured facts only.
package rollout

import (
	"bufio"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// SessionsDir is the rollout root inside a Codex home.
const SessionsDir = "sessions"

// FunctionCall is a persisted model tool call.
type FunctionCall struct {
	CallID    string
	Name      string
	Namespace string
	Arguments string
}

// FunctionCallOutput is the persisted output returned to the model for a call.
type FunctionCallOutput struct {
	CallID string
	// Text is the concatenated text content of the output.
	Text string
}

// ImageResult is a persisted image generation result.
type ImageResult struct {
	CallID    string
	Status    string
	Result    string
	SavedPath string
}

// Turn is one Turn of a session as the rollout recorded it.
type Turn struct {
	ID                string
	Model             string
	Effort            string
	MultiAgentVersion string
	Started           bool
	Completed         bool
	Aborted           bool
	Failed            bool
	LastAgentMessage  string
	FunctionCalls     []FunctionCall
	// FunctionCallOutputs are the persisted outputs, keyed by call id in
	// rollout order.
	FunctionCallOutputs []FunctionCallOutput
	// EncryptedReasoningCount counts persisted reasoning items that carried a
	// non-empty encrypted_content.
	EncryptedReasoningCount int
	AgentMessages           []string
	ImageResults            []ImageResult
	// SubAgentCompletions lists child thread ids whose completion this Turn
	// observed; a hint that corroborates the child's own rollout.
	SubAgentCompletions []string
	// ItemTypes counts persisted response_item types; a diagnostic, never a contract.
	ItemTypes map[string]int
}

// State summarizes the persisted lifecycle of the Turn for post-mortems.
func (t *Turn) State() string {
	switch {
	case t.Failed:
		return "failed"
	case t.Completed:
		return "completed"
	case t.Aborted:
		return "aborted"
	case t.Started:
		return "started"
	default:
		return "unknown"
	}
}

// FunctionCallCounts returns tool-call counts by name, a post-mortem hint.
func (t *Turn) FunctionCallCounts() map[string]int {
	counts := map[string]int{}
	for _, call := range t.FunctionCalls {
		name := call.Name
		if call.Namespace != "" {
			name = call.Namespace + "." + call.Name
		}
		counts[name]++
	}
	return counts
}

// FunctionCall finds the persisted call with the given id.
func (t *Turn) FunctionCall(callID string) (FunctionCall, bool) {
	for _, call := range t.FunctionCalls {
		if call.CallID == callID {
			return call, true
		}
	}
	return FunctionCall{}, false
}

// FunctionCallOutput finds the persisted output for the given call id.
func (t *Turn) FunctionCallOutput(callID string) (FunctionCallOutput, bool) {
	for _, output := range t.FunctionCallOutputs {
		if output.CallID == callID {
			return output, true
		}
	}
	return FunctionCallOutput{}, false
}

// Session is one rollout file.
type Session struct {
	Path           string
	ID             string
	ParentThreadID string
	ForkedFromID   string
	ModelProvider  string
	HistoryMode    string
	// SourceKind is the session_meta source discriminator ("cli", "vscode",
	// {"subagent": ...} becomes "subagent"), a hint only.
	SourceKind string
	// InheritedMetaIDs lists later session_meta records: a full-history fork
	// copies the parent's rollout head into the child file.
	InheritedMetaIDs []string
	Turns            []*Turn
	LineCount        int
}

// CompletedTurns returns the Turns this session recorded a terminal
// task_complete for, in rollout order.
func (s *Session) CompletedTurns() []*Turn {
	var turns []*Turn
	for _, turn := range s.Turns {
		if turn.Completed {
			turns = append(turns, turn)
		}
	}
	return turns
}

// Turn finds a Turn by id.
func (s *Session) Turn(id string) (*Turn, bool) {
	for _, turn := range s.Turns {
		if turn.ID == id {
			return turn, true
		}
	}
	return nil, false
}

// Graph is every session found under one Codex home.
type Graph struct {
	Sessions []*Session
}

// Session finds a session by thread id.
func (g *Graph) Session(id string) (*Session, bool) {
	for _, session := range g.Sessions {
		if session.ID == id {
			return session, true
		}
	}
	return nil, false
}

// Children returns the sessions whose session_meta names parent as their parent thread.
func (g *Graph) Children(parent string) []*Session {
	var children []*Session
	for _, session := range g.Sessions {
		if session.ParentThreadID != "" && session.ParentThreadID == parent {
			children = append(children, session)
		}
	}
	return children
}

// Scan reads every rollout under home/sessions. Compressed rollouts are
// rejected explicitly: a fresh validation home never has any, so meeting one
// means the assumption behind this reader changed.
func Scan(home string) (*Graph, error) {
	root := filepath.Join(home, SessionsDir)
	graph := &Graph{}
	err := filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.IsDir() {
			return nil
		}
		switch {
		case strings.HasSuffix(path, ".jsonl.zst"):
			return fmt.Errorf("compressed rollout is not supported: %s", filepath.Base(path))
		case strings.HasSuffix(path, ".jsonl"):
			session, err := ReadSession(path)
			if err != nil {
				return err
			}
			graph.Sessions = append(graph.Sessions, session)
		}
		return nil
	})
	if errors.Is(err, fs.ErrNotExist) {
		return graph, nil
	}
	if err != nil {
		return nil, err
	}
	return graph, nil
}

// WaitForTurnComplete polls the home until the rollout of thread contains a
// turn_complete record for turnID, or the timeout passes. Rollout persistence is
// asynchronous with respect to the app-server notification, so callers wait for
// the canonical record before reading it.
func WaitForTurnComplete(home, thread, turnID string, timeout time.Duration) bool {
	deadline := time.Now().Add(timeout)
	for {
		if graph, err := Scan(home); err == nil {
			if session, ok := graph.Session(thread); ok {
				if turn, ok := session.Turn(turnID); ok && (turn.Completed || turn.Aborted) {
					return true
				}
			}
		}
		if time.Now().After(deadline) {
			return false
		}
		time.Sleep(200 * time.Millisecond)
	}
}

type line struct {
	Timestamp string          `json:"timestamp"`
	Type      string          `json:"type"`
	Payload   json.RawMessage `json:"payload"`
}

type sessionMeta struct {
	ID             string          `json:"id"`
	ParentThreadID string          `json:"parent_thread_id"`
	ForkedFromID   string          `json:"forked_from_id"`
	ModelProvider  string          `json:"model_provider"`
	HistoryMode    string          `json:"history_mode"`
	Source         json.RawMessage `json:"source"`
}

type turnContext struct {
	TurnID            string `json:"turn_id"`
	Model             string `json:"model"`
	Effort            string `json:"effort"`
	MultiAgentVersion string `json:"multi_agent_version"`
}

type eventMsg struct {
	Type             string          `json:"type"`
	TurnID           string          `json:"turn_id"`
	LastAgentMessage *string         `json:"last_agent_message"`
	Error            json.RawMessage `json:"error"`
	Message          string          `json:"message"`
	CallID           string          `json:"call_id"`
	Status           string          `json:"status"`
	Result           string          `json:"result"`
	SavedPath        string          `json:"saved_path"`
	Item             json.RawMessage `json:"item"`
}

type responseItem struct {
	Type             string          `json:"type"`
	Role             string          `json:"role"`
	Name             string          `json:"name"`
	Namespace        string          `json:"namespace"`
	CallID           string          `json:"call_id"`
	Arguments        string          `json:"arguments"`
	EncryptedContent string          `json:"encrypted_content"`
	Output           json.RawMessage `json:"output"`
	Content          []struct {
		Type string `json:"type"`
		Text string `json:"text"`
	} `json:"content"`
}

// outputText flattens a function_call_output payload, which the recorder
// writes either as a plain string or as a list of content items.
func outputText(raw json.RawMessage) string {
	if len(raw) == 0 {
		return ""
	}
	var text string
	if json.Unmarshal(raw, &text) == nil {
		return text
	}
	var items []struct {
		Type string `json:"type"`
		Text string `json:"text"`
	}
	if json.Unmarshal(raw, &items) == nil {
		var joined strings.Builder
		for _, item := range items {
			if item.Type == "input_text" || item.Type == "output_text" || item.Type == "text" {
				joined.WriteString(item.Text)
			}
		}
		return joined.String()
	}
	var object struct {
		Content json.RawMessage `json:"content"`
	}
	if json.Unmarshal(raw, &object) == nil && len(object.Content) > 0 {
		return outputText(object.Content)
	}
	return ""
}

// ReadSession parses one rollout file.
func ReadSession(path string) (*Session, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	session := &Session{Path: path}
	var current *Turn
	turnByID := map[string]*Turn{}
	ensureTurn := func(id string) *Turn {
		if id != "" {
			if turn, ok := turnByID[id]; ok {
				return turn
			}
		}
		if current != nil && current.ID == "" && id != "" {
			current.ID = id
			turnByID[id] = current
			return current
		}
		turn := &Turn{ID: id, ItemTypes: map[string]int{}}
		session.Turns = append(session.Turns, turn)
		if id != "" {
			turnByID[id] = turn
		}
		return turn
	}
	currentTurn := func() *Turn {
		if current == nil {
			current = ensureTurn("")
		}
		return current
	}

	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, 0, 1024*1024), 64*1024*1024)
	for scanner.Scan() {
		raw := scanner.Bytes()
		if len(strings.TrimSpace(string(raw))) == 0 {
			continue
		}
		session.LineCount++
		var record line
		if err := json.Unmarshal(raw, &record); err != nil {
			return nil, fmt.Errorf("%s line %d: %w", filepath.Base(path), session.LineCount, err)
		}
		switch record.Type {
		case "session_meta":
			var meta sessionMeta
			if err := json.Unmarshal(record.Payload, &meta); err != nil {
				return nil, fmt.Errorf("%s session_meta: %w", filepath.Base(path), err)
			}
			if session.ID != "" {
				session.InheritedMetaIDs = append(session.InheritedMetaIDs, meta.ID)
				continue
			}
			session.ID = meta.ID
			session.ParentThreadID = meta.ParentThreadID
			session.ForkedFromID = meta.ForkedFromID
			session.ModelProvider = meta.ModelProvider
			session.HistoryMode = meta.HistoryMode
			session.SourceKind = sourceKind(meta.Source)
		case "turn_context":
			var context turnContext
			if err := json.Unmarshal(record.Payload, &context); err != nil {
				return nil, fmt.Errorf("%s turn_context: %w", filepath.Base(path), err)
			}
			turn := ensureTurn(context.TurnID)
			if context.TurnID == "" && current != nil && !current.Completed {
				turn = current
			}
			turn.Model = context.Model
			turn.Effort = context.Effort
			turn.MultiAgentVersion = context.MultiAgentVersion
			current = turn
		case "event_msg":
			var event eventMsg
			if err := json.Unmarshal(record.Payload, &event); err != nil {
				return nil, fmt.Errorf("%s event_msg: %w", filepath.Base(path), err)
			}
			switch event.Type {
			// The rollout keeps the legacy wire names task_started/task_complete
			// for TurnStarted/TurnComplete.
			case "task_started", "turn_started":
				turn := ensureTurn(event.TurnID)
				turn.Started = true
				current = turn
			case "task_complete", "turn_complete":
				turn := ensureTurn(event.TurnID)
				turn.Completed = true
				turn.Failed = len(event.Error) > 0 && string(event.Error) != "null"
				if event.LastAgentMessage != nil {
					turn.LastAgentMessage = *event.LastAgentMessage
				}
				current = turn
			case "turn_aborted":
				turn := ensureTurn(event.TurnID)
				turn.Aborted = true
				current = turn
			case "agent_message":
				currentTurn().addAgentMessage(event.Message)
			case "image_generation_end":
				currentTurn().ImageResults = append(currentTurn().ImageResults, ImageResult{
					CallID:    event.CallID,
					Status:    event.Status,
					Result:    event.Result,
					SavedPath: event.SavedPath,
				})
			case "item_completed":
				turn := currentTurn()
				if event.TurnID != "" {
					turn = ensureTurn(event.TurnID)
				}
				recordCompletedItem(turn, event.Item)
			}
		case "response_item":
			var item responseItem
			if err := json.Unmarshal(record.Payload, &item); err != nil {
				return nil, fmt.Errorf("%s response_item: %w", filepath.Base(path), err)
			}
			turn := currentTurn()
			turn.ItemTypes[item.Type]++
			switch item.Type {
			case "function_call":
				turn.FunctionCalls = append(turn.FunctionCalls, FunctionCall{
					CallID:    item.CallID,
					Name:      item.Name,
					Namespace: item.Namespace,
					Arguments: item.Arguments,
				})
			case "function_call_output":
				turn.FunctionCallOutputs = append(turn.FunctionCallOutputs, FunctionCallOutput{
					CallID: item.CallID,
					Text:   outputText(item.Output),
				})
			case "reasoning":
				if item.EncryptedContent != "" {
					turn.EncryptedReasoningCount++
				}
			case "message":
				if item.Role == "assistant" {
					for _, content := range item.Content {
						if content.Type == "output_text" {
							turn.addAgentMessage(content.Text)
						}
					}
				}
			}
		}
	}
	if err := scanner.Err(); err != nil {
		return nil, fmt.Errorf("%s: %w", filepath.Base(path), err)
	}
	if session.ID == "" {
		return nil, fmt.Errorf("%s has no session_meta", filepath.Base(path))
	}
	return session, nil
}

// recordCompletedItem handles paginated-history rollouts, which persist
// item_completed TurnItems (PascalCase `type`, camelCase members) instead of
// the legacy per-event records.
func recordCompletedItem(turn *Turn, raw json.RawMessage) {
	var item struct {
		Type          string `json:"type"`
		Kind          string `json:"kind"`
		ID            string `json:"id"`
		Status        string `json:"status"`
		Result        string `json:"result"`
		SavedPath     string `json:"savedPath"`
		AgentThreadID string `json:"agent_thread_id"`
		Content       []struct {
			Type string `json:"type"`
			Text string `json:"text"`
		} `json:"content"`
	}
	if json.Unmarshal(raw, &item) != nil {
		return
	}
	switch item.Type {
	case "SubAgentActivity":
		if item.Kind == "completed" && item.AgentThreadID != "" {
			turn.SubAgentCompletions = append(turn.SubAgentCompletions, item.AgentThreadID)
		}
	case "Extension":
		if strings.HasPrefix(item.Kind, "image_gen.") {
			turn.ImageResults = append(turn.ImageResults, ImageResult{
				CallID:    item.ID,
				Status:    item.Status,
				Result:    item.Result,
				SavedPath: item.SavedPath,
			})
		}
	case "ImageGeneration":
		turn.ImageResults = append(turn.ImageResults, ImageResult{
			CallID:    item.ID,
			Status:    item.Status,
			Result:    item.Result,
			SavedPath: item.SavedPath,
		})
	case "AgentMessage":
		var text strings.Builder
		for _, content := range item.Content {
			if content.Type == "Text" {
				text.WriteString(content.Text)
			}
		}
		turn.addAgentMessage(text.String())
	}
}

// addAgentMessage records a reply once even though legacy rollouts persist it
// both as an event and as the assistant response item.
func (t *Turn) addAgentMessage(text string) {
	if n := len(t.AgentMessages); n > 0 && t.AgentMessages[n-1] == text {
		return
	}
	t.AgentMessages = append(t.AgentMessages, text)
}

func sourceKind(raw json.RawMessage) string {
	if len(raw) == 0 {
		return ""
	}
	var text string
	if json.Unmarshal(raw, &text) == nil {
		return text
	}
	var object map[string]json.RawMessage
	if json.Unmarshal(raw, &object) == nil {
		for key := range object {
			return key
		}
	}
	return "unknown"
}
