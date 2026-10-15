package live

import (
	"context"
	"encoding/json"
	"errors"
	"sync"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// productClient is the shipped-scenario transport. Business proof stays in the
// existing turn, history and child observers.
type productClient interface {
	call(method string, params any, result any) error
	next() (frame, error)
	refuse(id json.RawMessage) error
	queued() []frame
	setNotification(func(frame) error)
}

func sdkShippedCatalog(ctx context.Context, options Options) (Evidence, error) {
	return withSDKFixture(ctx, options, nil, sdkFixtureHandlers{}, func(client *codexsdk.Client, _ string, evidence *Evidence) error {
		return requireSDKShippedCatalog(ctx, client, evidence)
	})
}

func sdkShippedSession(ctx context.Context, options Options, observe func(productClient, string, *Evidence) error) (Evidence, error) {
	transport := &sdkProductTransport{ctx: ctx, changed: make(chan struct{}, 1)}
	handlers := sdkFixtureHandlers{
		requests: transport.request, notifications: transport.typedNotification,
		retention: codexsdk.NotificationRetentionBudget{MaxCount: 8192, MaxBytes: 8 << 20},
		raw: &codexsdk.RawNotificationSubscription{
			Methods:                        []string{codexsdk.RawResponseItemCompletedExtension},
			EnableRawResponseItemExtension: true, Handler: transport.rawNotification,
		},
	}
	return withSDKFixture(ctx, options, nil, handlers, func(client *codexsdk.Client, cwd string, evidence *Evidence) error {
		transport.client = client
		if err := requireSDKShippedCatalog(ctx, client, evidence); err != nil {
			return err
		}
		if options.catalogOnly {
			return nil
		}
		experimental := true
		evidence.Threads = 1
		started, err := client.Threads().Start(ctx, protocolv2.ThreadStartParams{
			Model: protocolv2.Value(options.Model), ModelProvider: protocolv2.Value("grok"), CWD: protocolv2.Value(cwd),
			ExperimentalRawEvents: &experimental, ApprovalPolicy: protocolv2.Value(protocolv2.NewAskForApprovalNever()),
		})
		if err != nil {
			return shippedTransport(ctx, err)
		}
		if started.Model != options.Model || started.ModelProvider != "grok" || started.Thread.ModelProvider != "grok" ||
			started.Thread.ID == "" || started.CWD != cwd || started.Thread.CWD != cwd {
			return errors.New("live: fixture binding not established")
		}
		evidence.Bound, evidence.Stage = true, "thread_bound"
		return observe(transport, started.Thread.ID, evidence)
	})
}

func requireSDKShippedCatalog(ctx context.Context, client *codexsdk.Client, evidence *Evidence) error {
	limit := uint32(100)
	listed, err := client.Models().List(ctx, protocolv2.ModelListParams{Limit: protocolv2.Value(limit)})
	if err != nil {
		return errors.New("live: complete shipped catalog mismatch")
	}
	expected, err := shippedModelList()
	if err != nil || !sameShippedCatalog(listed, expected) {
		return errors.New("live: complete shipped catalog mismatch")
	}
	evidence.CatalogModels, evidence.ShippedCatalog = len(listed.Data), true
	evidence.Stage = "shipped_catalog_verified"
	return nil
}

// sameShippedCatalog checks material catalog semantics. Omitted or null optional
// DTO fields are not a mismatch; order, identity, default, efforts, modalities,
// multi-agent version and pagination are.
func sameShippedCatalog(listed protocolv2.ModelListResponse, expected map[string]any) bool {
	if listed.NextCursor != nil && listed.NextCursor.Value != nil {
		return false
	}
	rows, _ := expected["data"].([]any)
	if len(listed.Data) != len(rows) {
		return false
	}
	for index, model := range listed.Data {
		row, _ := rows[index].(map[string]any)
		if model.ID != shippedString(row["id"]) || model.Model != shippedString(row["model"]) || model.DisplayName != shippedString(row["displayName"]) ||
			model.Description != shippedString(row["description"]) || model.Hidden != shippedBool(row["hidden"]) || model.IsDefault != shippedBool(row["isDefault"]) ||
			string(model.DefaultReasoningEffort) != shippedString(row["defaultReasoningEffort"]) || !shippedEffortsMatch(model.SupportedReasoningEfforts, row["supportedReasoningEfforts"]) ||
			!shippedModalitiesMatch(model.InputModalities, row["inputModalities"]) || shippedMultiAgent(model) != shippedString(row["multiAgentVersion"]) {
			return false
		}
	}
	return true
}

func shippedString(value any) string {
	text, _ := value.(string)
	return text
}

func shippedBool(value any) bool {
	flag, _ := value.(bool)
	return flag
}

func shippedMultiAgent(model protocolv2.Model) string {
	if model.MultiAgentVersion == nil || model.MultiAgentVersion.Value == nil {
		return ""
	}
	return string(*model.MultiAgentVersion.Value)
}

func shippedEffortsMatch(got []protocolv2.ReasoningEffortOption, want any) bool {
	rows, ok := want.([]any)
	if !ok || len(got) != len(rows) {
		return false
	}
	for index, effort := range got {
		row, _ := rows[index].(map[string]any)
		if string(effort.ReasoningEffort) != shippedString(row["reasoningEffort"]) || effort.Description != shippedString(row["description"]) {
			return false
		}
	}
	return true
}

func shippedModalitiesMatch(got *[]protocolv2.InputModality, want any) bool {
	rows, ok := want.([]any)
	if got == nil || !ok || len(*got) != len(rows) {
		return false
	}
	for index, modality := range *got {
		if string(modality) != shippedString(rows[index]) {
			return false
		}
	}
	return true
}

func shippedTransport(ctx context.Context, err error) error {
	if ctx.Err() != nil {
		return errors.New("live: protocol ended before proof completion")
	}
	var protocol *codexsdk.ProtocolError
	if errors.As(err, &protocol) {
		return errors.New("live: protocol request failed")
	}
	if probeDone(err) {
		return errors.New("live: protocol ended before proof completion")
	}
	return failedObservation("sdk_observation_failed", "live: SDK observation did not complete")
}

type sdkProductTransport struct {
	ctx        context.Context
	client     *codexsdk.Client
	mu         sync.Mutex
	pending    []frame
	changed    chan struct{}
	clientDone <-chan struct{}
	cause      error
	onNote     func(frame) error
}

func (transport *sdkProductTransport) setNotification(fn func(frame) error) {
	transport.mu.Lock()
	transport.onNote = fn
	transport.mu.Unlock()
}

func (transport *sdkProductTransport) queued() []frame {
	transport.mu.Lock()
	defer transport.mu.Unlock()
	out := make([]frame, len(transport.pending))
	copy(out, transport.pending)
	return out
}

func (transport *sdkProductTransport) call(method string, params any, result any) error {
	raw, err := json.Marshal(params)
	if err != nil {
		return errors.New("live: invalid protocol request")
	}
	switch method {
	case "turn/start":
		var decoded struct {
			ThreadID string `json:"threadId"`
			Effort   string `json:"effort"`
			Input    []struct {
				Text string `json:"text"`
			} `json:"input"`
		}
		if json.Unmarshal(raw, &decoded) != nil || decoded.ThreadID == "" || len(decoded.Input) != 1 {
			return errors.New("live: invalid protocol request")
		}
		request := protocolv2.TurnStartParams{
			ThreadID: decoded.ThreadID,
			Input:    []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: decoded.Input[0].Text})},
		}
		if decoded.Effort != "" {
			request.Effort = protocolv2.Value(protocolv2.ReasoningEffort(decoded.Effort))
		}
		started, err := transport.client.Turns().Start(transport.ctx, request)
		if err != nil {
			return shippedTransport(transport.ctx, err)
		}
		encoded, err := json.Marshal(struct {
			Turn protocolv2.Turn `json:"turn"`
		}{Turn: started.Turn})
		if err != nil || json.Unmarshal(encoded, result) != nil {
			return errors.New("live: invalid product completion")
		}
		return nil
	case "thread/read":
		var decoded struct {
			ThreadID string `json:"threadId"`
		}
		if json.Unmarshal(raw, &decoded) != nil || decoded.ThreadID == "" {
			return errors.New("live: invalid protocol request")
		}
		include := true
		settled, err := transport.client.Threads().Read(transport.ctx, protocolv2.ThreadReadParams{ThreadID: decoded.ThreadID, IncludeTurns: &include})
		if err != nil {
			return shippedTransport(transport.ctx, err)
		}
		encoded, err := json.Marshal(struct {
			Thread protocolv2.Thread `json:"thread"`
		}{Thread: settled.Thread})
		if err != nil || json.Unmarshal(encoded, result) != nil {
			return errors.New("live: invalid product completion")
		}
		return nil
	default:
		return errors.New("live: invalid protocol request")
	}
}

func (transport *sdkProductTransport) next() (frame, error) {
	for {
		transport.mu.Lock()
		cause, pending, clientDone := transport.cause, len(transport.pending), transport.clientDone
		var message frame
		if cause == nil && pending > 0 {
			message = transport.pending[0]
			transport.pending = transport.pending[1:]
		}
		transport.mu.Unlock()
		if cause != nil {
			return frame{}, cause
		}
		if pending > 0 {
			return message, nil
		}
		select {
		case <-transport.changed:
		case <-clientDone:
			return frame{}, errors.New("live: protocol ended before proof completion")
		case <-transport.ctx.Done():
			return frame{}, errors.New("live: protocol ended before proof completion")
		}
	}
}

func (transport *sdkProductTransport) refuse(id json.RawMessage) error {
	if len(id) == 0 {
		return errors.New("live: unsupported server request")
	}
	return failedObservation("unsupported_server_request", "live: unsupported server request")
}

func (transport *sdkProductTransport) request(ctx context.Context, _ protocolv2.ServerRequest) (codexsdk.ServerRequestResponse, error) {
	transport.note(ctx, errors.New("live: unsupported server request"))
	return codexsdk.ServerRequestResponse{}, nil
}

func (transport *sdkProductTransport) rawNotification(ctx context.Context, notification codexsdk.RawNotification) error {
	if notification.Method != codexsdk.RawResponseItemCompletedExtension {
		return nil
	}
	raw, err := notification.Envelope.MarshalJSON()
	return transport.observe(ctx, notification.Method, raw, err)
}

func (transport *sdkProductTransport) typedNotification(ctx context.Context, notification protocolv2.ServerNotification) error {
	method := ""
	switch notification.Kind() {
	case protocolv2.ServerNotificationKindItemStarted:
		method = "item/started"
	case protocolv2.ServerNotificationKindItemCompleted:
		method = "item/completed"
	case protocolv2.ServerNotificationKindTurnStarted:
		method = "turn/started"
	case protocolv2.ServerNotificationKindTurnCompleted:
		method = "turn/completed"
	case protocolv2.ServerNotificationKindThreadClosed:
		method = "thread/closed"
	default:
		return nil
	}
	raw, err := json.Marshal(notification)
	return transport.observe(ctx, method, raw, err)
}

func (transport *sdkProductTransport) observe(ctx context.Context, method string, raw []byte, marshalErr error) error {
	transport.mu.Lock()
	defer transport.mu.Unlock()
	transport.noteCallback(ctx)
	var envelope struct {
		Params json.RawMessage `json:"params"`
	}
	if marshalErr != nil || json.Unmarshal(raw, &envelope) != nil || len(envelope.Params) == 0 {
		transport.cause = errors.New("live: invalid product completion")
		transport.signal()
		return nil
	}
	message := frame{Method: method, Params: envelope.Params, size: len(raw)}
	if transport.onNote != nil {
		if err := transport.onNote(message); err != nil {
			transport.cause = err
		}
	}
	transport.pending = append(transport.pending, message)
	transport.signal()
	return nil
}

func (transport *sdkProductTransport) note(ctx context.Context, err error) {
	transport.mu.Lock()
	defer transport.mu.Unlock()
	transport.noteCallback(ctx)
	if transport.cause == nil {
		transport.cause = err
	}
	transport.signal()
}

func (transport *sdkProductTransport) noteCallback(ctx context.Context) {
	if transport.clientDone == nil && ctx != nil {
		transport.clientDone = ctx.Done()
	}
}

func (transport *sdkProductTransport) signal() {
	select {
	case transport.changed <- struct{}{}:
	default:
	}
}
