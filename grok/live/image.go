package live

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
)

// ImageEvidence contains bounded observations of the explicit-binary scenario.
// It does not attest to backend visual semantics, packaging or Responses egress.
type ImageEvidence struct {
	Evidence
	TurnCompleted, HistorySelected          bool
	Images, Artifacts                       int
	FirstImageSHA256, EditedImageSHA256     string
	FirstHistorySHA256, EditedHistorySHA256 string
}

// ImageGenerationEdit submits one generation and one same-thread history edit.
// Neither semantic turn is retried, including when only partial evidence exists.
func ImageGenerationEdit(ctx context.Context, options Options) (ImageEvidence, error) {
	options.imageFixture = true
	evidence := ImageEvidence{}
	base, err := runFixture(ctx, options, nil, map[string]any{"experimentalRawEvents": true}, func(server *appServer, threadID string, base *Evidence) error {
		probe := imageProbe{ctx: ctx, server: server, threadID: threadID, root: filepath.Join(filepath.Dir(server.cmd.Dir), "generated_images"), base: base, evidence: &evidence, calls: map[string]*imageCall{}}
		if err := probe.observe("Generate an image of a blue circle on a plain white background."); err != nil {
			return err
		}
		probe.prior, probe.previousID = probe.newest, probe.turnID
		evidence.FirstImageSHA256, evidence.FirstHistorySHA256 = probe.prior.digest, probe.prior.output
		if err := probe.observe("Edit the image you just generated so the circle is green while keeping the plain white background."); err != nil {
			return err
		}
		if !evidence.HistorySelected || probe.newest == nil || !probe.newest.fromPrior || probe.newest.digest == probe.prior.digest {
			return errors.New("live: distinct history-derived edit unavailable")
		}
		// Recheck the generation artifact after the edit: both results remain accessible.
		if _, err := inspectImage(probe.root, probe.prior.path, probe.prior.digest); err != nil {
			return err
		}
		if ctx.Err() != nil {
			return errors.New("live: image observation deadline exceeded")
		}
		evidence.EditedImageSHA256, evidence.EditedHistorySHA256 = probe.newest.digest, probe.newest.output
		base.Completed, base.Stage = true, "image_edit_verified"
		return nil
	})
	evidence.Evidence = base
	return evidence, err
}

type imageItem struct {
	ID        string          `json:"id"`
	Type      string          `json:"type"`
	Name      string          `json:"name"`
	Namespace *string         `json:"namespace"`
	CallID    string          `json:"call_id"`
	Arguments string          `json:"arguments"`
	Output    json.RawMessage `json:"output"`
	Content   json.RawMessage `json:"content"`
	Status    string          `json:"status"`
	Result    string          `json:"result"`
	SavedPath string          `json:"savedPath"`
	Failure   json.RawMessage `json:"failure"`
}
type imageTurn struct {
	ID     string          `json:"id"`
	Status string          `json:"status"`
	Error  json.RawMessage `json:"error"`
	Items  []imageItem     `json:"items"`
}

// output is the canonical prepared-rendition digest; digest is the public image
// payload digest. Stock history preparation may resize/re-encode between them.
type imageCall struct {
	turnID, arguments, digest, output, path, terminal, failure, ineligible string
	fromPrior, called, started, public, settled, verified                  bool
}
type imageProbe struct {
	ctx                                context.Context
	server                             *appServer
	threadID, turnID, previousID, root string
	base                               *Evidence
	evidence                           *ImageEvidence
	calls                              map[string]*imageCall
	newest, prior                      *imageCall
	existing                           map[string]bool
	completed                          bool
	frames, bytes                      int
}

func (probe *imageProbe) observe(prompt string) error {
	var err error
	probe.existing, err = imagePaths(probe.root)
	if err != nil {
		return err
	}
	probe.base.Turns++
	probe.evidence.TurnCompleted, probe.completed = false, false
	probe.frames, probe.bytes = 0, 0
	var started struct {
		Turn imageTurn `json:"turn"`
	}
	if err := probe.server.call("turn/start", map[string]any{"threadId": probe.threadID, "input": []any{map[string]any{"type": "text", "text": prompt, "textElements": []any{}}}}, &started); err != nil {
		return err
	}
	if started.Turn.ID == "" || started.Turn.ID == probe.previousID {
		return errors.New("live: image turn identity unavailable")
	}
	probe.turnID, probe.base.Stage = started.Turn.ID, "image_turn_submitted"
	if started.Turn.Status != "inProgress" {
		if err := probe.terminal(started.Turn, false); err != nil {
			return err
		}
	}
	for !probe.completed {
		message, err := probe.server.next()
		if err != nil {
			return err
		}
		if err := probe.notification(message); err != nil {
			return err
		}
	}
	// The live ThreadStore read is the settled public history, not a home-directory
	// search. It also fences queued notifications without resubmitting a turn.
	var read struct {
		Thread struct {
			ID    string      `json:"id"`
			Turns []imageTurn `json:"turns"`
		} `json:"thread"`
	}
	if err := probe.server.call("thread/read", map[string]any{"threadId": probe.threadID, "includeTurns": true}, &read); err != nil {
		return err
	}
	for len(probe.server.pending) != 0 {
		message, err := probe.server.next()
		if err != nil {
			return err
		}
		if err := probe.notification(message); err != nil {
			return err
		}
	}
	if read.Thread.ID != probe.threadID || len(read.Thread.Turns) != probe.base.Turns {
		return errors.New("live: settled image thread identity unavailable")
	}
	for i, turn := range read.Thread.Turns {
		expected := probe.turnID
		if i == 0 && probe.previousID != "" {
			expected = probe.previousID
		}
		if turn.ID != expected || turn.Status != "completed" || !nullJSON(turn.Error) {
			return errors.New("live: settled image turn did not complete")
		}
		if turn.ID == probe.turnID {
			if err := probe.terminal(turn, true); err != nil {
				return err
			}
		} else {
			found := false
			for _, item := range turn.Items {
				if item.Type == "imageGeneration" && item.ID != "" {
					call := probe.calls[item.ID]
					if call == nil || call.turnID != turn.ID || !sameImageItem(call, item) {
						return errors.New("live: prior image history changed")
					}
					found = found || call == probe.prior
				}
			}
			if !found {
				return errors.New("live: prior image history unavailable")
			}
		}
	}
	found := false
	for _, call := range probe.calls {
		if probe.ctx.Err() != nil {
			return errors.New("live: image observation deadline exceeded")
		}
		if call.turnID != probe.turnID {
			continue
		}
		if !call.called || call.output == "" {
			return errors.New("live: image call, output or terminal evidence missing")
		}
		// Argument/reference validation fails before the extension emits any
		// image item. A correlated text error is diagnostic, never an image.
		if !call.started && !call.public && strings.HasPrefix(call.output, "text:") {
			continue
		}
		if !call.public || !call.settled {
			return errors.New("live: image call, output or terminal evidence missing")
		}
		if call.ineligible != "" {
			return errors.New(call.ineligible)
		}
		if call.terminal == "failed" && strings.HasPrefix(call.output, "text:") {
			continue
		}
		if call.terminal != "completed" || strings.HasPrefix(call.output, "text:") {
			return errors.New("live: image call, output or terminal evidence missing")
		}
		if probe.existing[call.path] {
			return errors.New("live: image artifact predates turn")
		}
		if _, err := inspectImage(probe.root, call.path, call.digest); err != nil {
			return err
		}
		if !call.verified {
			probe.evidence.Artifacts++
			call.verified = true
		}
		found = true
	}
	if !found || probe.newest == nil || probe.newest.turnID != probe.turnID || !probe.newest.verified {
		return errors.New("live: completed canonical image unavailable")
	}
	probe.base.Stage = "image_generation_verified"
	return nil
}

func (probe *imageProbe) notification(message frame) error {
	if probe.ctx.Err() != nil {
		return errors.New("live: image observation deadline exceeded")
	}
	probe.frames++
	probe.bytes += message.size
	if probe.frames > 4096 || probe.bytes > 64<<20 {
		return errors.New("live: image evidence budget exceeded")
	}
	if len(message.ID) != 0 {
		return probe.server.refuse(message.ID)
	}
	if message.Method != "rawResponseItem/completed" && message.Method != "item/completed" && message.Method != "item/started" && message.Method != "turn/completed" {
		return nil
	}
	var event struct {
		ThreadID string    `json:"threadId"`
		TurnID   string    `json:"turnId"`
		Turn     imageTurn `json:"turn"`
		Item     imageItem `json:"item"`
	}
	if json.Unmarshal(message.Params, &event) != nil {
		return errors.New("live: invalid image evidence")
	}
	if event.ThreadID != probe.threadID {
		return errors.New("live: unrelated image thread evidence")
	}
	if message.Method == "turn/completed" {
		return probe.terminal(event.Turn, false)
	}
	if event.TurnID != probe.turnID {
		return errors.New("live: unrelated image turn evidence")
	}
	if message.Method == "rawResponseItem/completed" {
		return probe.raw(event.Item)
	}
	if message.Method == "item/started" {
		if event.Item.Type == "imageGeneration" {
			call, err := probe.record(event.Item.ID)
			if err != nil {
				return err
			}
			call.started = true
		}
		return nil
	}
	return probe.public(event.Item, false)
}

func (probe *imageProbe) terminal(turn imageTurn, settled bool) error {
	if turn.ID != probe.turnID || turn.Status != "completed" || !nullJSON(turn.Error) {
		return errors.New("live: image turn did not complete successfully")
	}
	probe.completed, probe.evidence.TurnCompleted = true, true
	if probe.previousID == "" {
		probe.base.FirstCompleted = true
	}
	probe.base.Stage = "image_turn_completed"
	for _, item := range turn.Items {
		if err := probe.public(item, settled); err != nil {
			return err
		}
	}
	return nil
}

func (probe *imageProbe) record(id string) (*imageCall, error) {
	if id == "" || len(id) > 1024 {
		return nil, errors.New("live: image call identity unavailable")
	}
	call := probe.calls[id]
	if call == nil {
		if len(probe.calls) >= 64 {
			return nil, errors.New("live: image call budget exceeded")
		}
		call = &imageCall{turnID: probe.turnID}
		probe.calls[id] = call
	}
	if call.turnID != probe.turnID {
		return nil, errors.New("live: stale image call identity")
	}
	return call, nil
}

func (probe *imageProbe) raw(item imageItem) error {
	switch item.Type {
	case "function_call":
		if item.Name != "imagegen" || item.Namespace == nil || *item.Namespace != "image_gen" {
			return errors.New("live: unexpected image tool identity")
		}
		call, err := probe.record(item.CallID)
		if err != nil {
			return err
		}
		if len(item.Arguments) > 4096 {
			return errors.New("live: invalid image arguments")
		}
		hash := editDigest([]byte(item.Arguments))
		if call.called {
			if call.arguments != hash {
				return errors.New("live: conflicting image call")
			}
			return nil
		}
		call.called, call.arguments = true, hash
		probe.base.ToolCalls++
		args, err := parseImageArguments(item.Arguments)
		if err != nil {
			call.ineligible = err.Error()
			return nil
		}
		if probe.previousID == "" {
			if args.Count != nil || len(args.Paths) != 0 {
				call.ineligible = "live: unexpected generation reference"
				return nil
			}
		} else {
			selected := probe.newest
			if len(args.Paths) == 1 && args.Count == nil && args.Paths[0] == probe.prior.path {
				selected = probe.prior
			} else if len(args.Paths) != 0 || args.Count == nil || *args.Count != 1 {
				call.ineligible = "live: exact image history selector unavailable"
				return nil
			}
			if selected == nil || (selected != probe.prior && !selected.fromPrior) || !selected.verified && selected != probe.newest {
				call.ineligible = "live: selected prior image unavailable"
				return nil
			}
		}
	case "function_call_output":
		call, err := probe.record(item.CallID)
		if err != nil {
			return err
		}
		hash, err := imageOutputDigest(item.Output)
		if err != nil {
			return err
		}
		if call.output != "" {
			if call.output != hash {
				return errors.New("live: conflicting image output")
			}
			return nil
		}
		if !call.called {
			return errors.New("live: image output precedes canonical call")
		}
		if call.ineligible != "" && !strings.HasPrefix(hash, "text:") {
			return errors.New(call.ineligible)
		}
		call.output = hash
		if !strings.HasPrefix(hash, "text:") {
			call.fromPrior = probe.previousID != ""
			probe.evidence.HistorySelected = probe.evidence.HistorySelected || call.fromPrior
			probe.newest = call
		}
	case "message":
		var content []struct {
			Type string `json:"type"`
		}
		if len(item.Content) != 0 && json.Unmarshal(item.Content, &content) != nil {
			return errors.New("live: invalid canonical image history")
		}
		for _, entry := range content {
			if entry.Type == "input_image" {
				return errors.New("live: unrelated canonical history image")
			}
		}
	case "compaction", "compaction_trigger", "context_compaction", "image_generation_call", "custom_tool_call", "custom_tool_call_output", "local_shell_call", "tool_search_call":
		return errors.New("live: unsupported image history mutation")
	}
	return nil
}

func (probe *imageProbe) public(item imageItem, settled bool) error {
	if probe.ctx.Err() != nil {
		return errors.New("live: image observation deadline exceeded")
	}
	if item.Type != "imageGeneration" {
		return nil
	}
	call, err := probe.record(item.ID)
	if err != nil {
		return err
	}
	if call.ineligible != "" {
		return errors.New(call.ineligible)
	}
	if call.public {
		if !sameImageItem(call, item) {
			return errors.New("live: conflicting image terminal")
		}
	} else {
		if item.Status == "failed" && item.Result == "" && item.SavedPath == "" {
			call.terminal, call.public, call.settled = "failed", true, settled
			call.failure = imageFailureDigest(item.Failure)
			return nil
		}
		if item.Status != "completed" || !nullJSON(item.Failure) || item.SavedPath == "" {
			return errors.New("live: image did not complete successfully")
		}
		payload, err := imagePayload(item.Result)
		if err != nil {
			return err
		}
		call.digest, call.path, call.terminal, call.public = editDigest(payload), item.SavedPath, item.Status, true
		probe.evidence.Images++
	}
	call.settled = call.settled || settled
	return nil
}

func sameImageItem(call *imageCall, item imageItem) bool {
	if call.terminal == "failed" {
		return item.Status == "failed" && item.Result == "" && item.SavedPath == "" && call.failure == imageFailureDigest(item.Failure)
	}
	data, err := imagePayload(item.Result)
	return err == nil && item.Status == call.terminal && item.SavedPath == call.path && nullJSON(item.Failure) && editDigest(data) == call.digest
}
func nullJSON(value json.RawMessage) bool {
	return len(value) == 0 || strings.TrimSpace(string(value)) == "null"
}

func imageFailureDigest(raw json.RawMessage) string {
	if nullJSON(raw) {
		return ""
	}
	var value any
	_ = json.Unmarshal(raw, &value)
	canonical, _ := json.Marshal(value)
	return editDigest(canonical)
}
