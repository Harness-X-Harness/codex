// Package driver runs real tasks against one packaged Grokex app-server through
// the exact protocol client (codexsdk). It owns process lifecycle and Turn
// deadlines; what the model did is read afterwards from the session rollouts.
package driver

import (
	"context"
	"errors"
	"fmt"
	"os"
	"sort"
	"strings"
	"time"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// Model is the only Grok model the Live contract exercises.
const Model = "grok-4.6"

// Provider is the Grok model provider id in the release-bundled catalog.
const Provider = "grok"

const notificationQueueCapacity = 1 << 16

// TurnRun is the externally observable result of one Turn: what the
// app-server delivered to the client, independent of what it persisted.
type TurnRun struct {
	ThreadID        string
	TurnID          string
	Status          string
	FinalResponse   string
	Duration        time.Duration
	DeadlineExpired bool
	// FinalResponseSource says where FinalResponse came from:
	// "final_answer_phase" when codexsdk derived it from an agentMessage whose
	// phase is final_answer, "last_agent_message" when the delivered Turn
	// carried agentMessage items without a phase (Grok replies do not carry
	// the stock phase marker).
	FinalResponseSource string
	// NotificationKinds and ItemKinds are hints for post-mortems.
	NotificationKinds map[string]int
	ItemKinds         map[string]int
}

// Completed reports whether the app-server delivered a completed Turn.
func (r TurnRun) Completed() bool {
	return !r.DeadlineExpired && r.Status == string(protocolv2.TurnStatusCompleted)
}

// Driver is one app-server process bound to a fresh Codex home.
type Driver struct {
	client    *codexsdk.Client
	Home      string
	Workspace string
	// Requests answers and counts app-server requests for the whole process.
	Requests *ServerRequests
}

// Start launches `<binary> app-server --strict-config --listen stdio://` with
// CODEX_HOME=home. The environment is set on this process because the exact
// client inherits it; the validator process exists only for this run. tool, when
// set, is the one dynamic tool the validator answers.
func Start(binary, home, workspace, clientVersion string, tool *DynamicTool) (*Driver, error) {
	if err := os.Setenv("CODEX_HOME", home); err != nil {
		return nil, err
	}
	if err := os.Setenv("NO_COLOR", "1"); err != nil {
		return nil, err
	}
	experimental := true
	requests := NewServerRequests(tool)
	client, err := codexsdk.New(codexsdk.ClientOptions{
		CWD:                  workspace,
		Command:              []string{binary, "app-server", "--strict-config", "--listen", "stdio://"},
		ServerRequestHandler: requests.Handler,
		// The default 64-slot queue overflowed on the image edit Turn (streamed
		// reasoning/message deltas plus base64 image items) and failed the
		// client with ErrNotificationBackpressure; this validator observes one
		// Turn at a time and can afford a deep queue.
		NotificationQueueCapacity: notificationQueueCapacity,
		Initialize: protocolv2.InitializeParams{
			ClientInfo: protocolv2.ClientInfo{Name: "grokex-live-validator", Version: clientVersion},
			Capabilities: protocolv2.Value(protocolv2.InitializeCapabilities{
				ExperimentalAPI: &experimental,
			}),
		},
	})
	if err != nil {
		return nil, fmt.Errorf("start app-server: %w", err)
	}
	return &Driver{client: client, Home: home, Workspace: workspace, Requests: requests}, nil
}

// Close shuts the app-server down normally. Callers wait for the rollout
// record they need before closing.
func (d *Driver) Close() error {
	return d.client.Close()
}

// Catalog is what model/list says about the Grok model.
type Catalog struct {
	ModelListed       bool
	MultiAgentVersion string
	ReasoningEfforts  []string
}

// VerifyCatalog reads the release-bundled catalog through the public API.
func (d *Driver) VerifyCatalog(ctx context.Context) (Catalog, error) {
	response, err := d.client.Models().List(ctx, protocolv2.ModelListParams{})
	if err != nil {
		return Catalog{}, fmt.Errorf("model/list: %w", err)
	}
	for _, model := range response.Data {
		if model.ID != Model {
			continue
		}
		catalog := Catalog{ModelListed: true}
		if model.MultiAgentVersion != nil && model.MultiAgentVersion.Value != nil {
			catalog.MultiAgentVersion = string(*model.MultiAgentVersion.Value)
		}
		for _, effort := range model.SupportedReasoningEfforts {
			catalog.ReasoningEfforts = append(catalog.ReasoningEfforts, string(effort.ReasoningEffort))
		}
		sort.Strings(catalog.ReasoningEfforts)
		return catalog, nil
	}
	return Catalog{}, fmt.Errorf("model/list does not list %s", Model)
}

// TurnRequest describes one user Turn.
type TurnRequest struct {
	Prompt   string
	Effort   string
	Deadline time.Duration
}

// params builds the Turn parameters; Turn.ThreadID stays empty because the exact
// client composes it from the thread/start or thread/resume response.
func (r TurnRequest) params() protocolv2.TurnStartParams {
	params := protocolv2.TurnStartParams{
		Input: []protocolv2.UserInput{
			protocolv2.NewUserInputText(protocolv2.UserInputText{Text: r.Prompt}),
		},
	}
	if r.Effort != "" {
		params.Effort = protocolv2.Value(protocolv2.ReasoningEffort(r.Effort))
	}
	return params
}

// StartThread starts a Grok Thread in the workspace and runs the first Turn.
// The configured dynamic tool, if any, is offered on the Thread.
func (d *Driver) StartThread(ctx context.Context, request TurnRequest) (TurnRun, error) {
	started := time.Now()
	runCtx, cancel := context.WithTimeout(ctx, request.Deadline)
	defer cancel()
	thread := protocolv2.ThreadStartParams{
		CWD:           protocolv2.Value(d.Workspace),
		Model:         protocolv2.Value(Model),
		ModelProvider: protocolv2.Value(Provider),
	}
	if d.Requests.tool != nil {
		thread.DynamicTools = protocolv2.Value([]protocolv2.DynamicToolSpec{d.Requests.tool.Spec()})
	}
	stream, err := d.client.ThreadRunner().StartStream(ctx, codexsdk.StartThreadRunRequest{
		Thread: thread,
		Turn:   request.params(),
	})
	if err != nil {
		return TurnRun{}, fmt.Errorf("thread/start: %w", err)
	}
	result, waitErr := stream.Wait(runCtx)
	run := d.finish(ctx, result.Start.Thread.ID, result.Run, started, waitErr)
	return run, classify(run, waitErr)
}

// ContinueThread runs one more Turn on an existing Thread.
func (d *Driver) ContinueThread(ctx context.Context, threadID string, request TurnRequest) (TurnRun, error) {
	started := time.Now()
	runCtx, cancel := context.WithTimeout(ctx, request.Deadline)
	defer cancel()
	// codexsdk composes Turn.ThreadID from the resume response and rejects a
	// caller-supplied one ("composition-owned").
	stream, err := d.client.ThreadRunner().ResumeStream(ctx, codexsdk.ResumeThreadRunRequest{
		Thread: protocolv2.ThreadResumeParams{ThreadID: threadID},
		Turn:   request.params(),
	})
	if err != nil {
		return TurnRun{}, fmt.Errorf("thread/resume: %w", err)
	}
	result, waitErr := stream.Wait(runCtx)
	run := d.finish(ctx, threadID, result.Run, started, waitErr)
	return run, classify(run, waitErr)
}

func (d *Driver) finish(ctx context.Context, threadID string, result codexsdk.ThreadRunResult, started time.Time, waitErr error) TurnRun {
	run := TurnRun{
		ThreadID:          threadID,
		TurnID:            result.Turn.ID,
		Status:            string(result.Turn.Status),
		FinalResponse:     result.FinalResponse,
		Duration:          time.Since(started),
		DeadlineExpired:   errors.Is(waitErr, context.DeadlineExceeded),
		NotificationKinds: map[string]int{},
		ItemKinds:         map[string]int{},
	}
	for _, notification := range result.Notifications {
		run.NotificationKinds[string(notification.Kind())]++
	}
	for _, item := range result.Turn.Items {
		run.ItemKinds[string(item.Kind())]++
	}
	switch {
	case run.FinalResponse != "":
		run.FinalResponseSource = "final_answer_phase"
	case run.Status == string(protocolv2.TurnStatusCompleted):
		run.FinalResponse = lastAgentMessage(result.Turn.Items)
		if run.FinalResponse != "" {
			run.FinalResponseSource = "last_agent_message"
		}
	}
	if run.DeadlineExpired && threadID != "" && run.TurnID != "" {
		// Stop paid work and let the app-server persist the aborted Turn; the
		// rollout then shows what the model was doing when the deadline hit.
		interruptCtx, cancel := context.WithTimeout(ctx, 20*time.Second)
		defer cancel()
		_, _ = d.client.Turns().Interrupt(interruptCtx, protocolv2.TurnInterruptParams{
			ThreadID: threadID,
			TurnID:   run.TurnID,
		})
	}
	return run
}

// ErrDeadline marks a Turn that did not reach a terminal state in time.
var ErrDeadline = errors.New("turn deadline expired")

// lastAgentMessage returns the text of the last non-empty agentMessage item.
// codexsdk only recognizes replies whose phase is final_answer; the Grok
// graft delivers agentMessage items without a phase, so the delivered reply is
// the last message the app-server reported for the completed Turn.
func lastAgentMessage(items []protocolv2.ThreadItem) string {
	for index := len(items) - 1; index >= 0; index-- {
		if message, ok := items[index].AsAgentMessage(); ok && message.Text != "" {
			return message.Text
		}
	}
	return ""
}

// classify maps the exact client's wait error onto the validator's classes. A
// completed Turn that codexsdk rejected only for lacking a final_answer phase
// is a completed Turn: the delivered reply is read from the Turn items.
func classify(run TurnRun, waitErr error) error {
	switch {
	case waitErr == nil:
		return nil
	case errors.Is(waitErr, context.DeadlineExceeded):
		return ErrDeadline
	case run.Status == string(protocolv2.TurnStatusCompleted) &&
		strings.Contains(waitErr.Error(), "without final_answer agent message"):
		return nil
	default:
		return waitErr
	}
}
