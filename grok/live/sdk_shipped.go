package live

import (
	"context"
	"encoding/json"
	"errors"
	"sync"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func sdkShippedCatalog(ctx context.Context, options Options) (Evidence, error) {
	return withSDKFixture(ctx, options, nil, sdkFixtureHandlers{}, func(client *codexsdk.Client, _ string, evidence *Evidence) error {
		return requireSDKShippedCatalog(ctx, client, evidence)
	})
}

func sdkShippedSession(ctx context.Context, options Options, observe func(*sdkProductObserver, string, *Evidence) error) (Evidence, error) {
	transport := &sdkProductObserver{ctx: ctx, changed: make(chan struct{}, 1)}
	handlers := sdkFixtureHandlers{
		requests: transport.request, notifications: transport.typedNotification, afterClose: transport.afterClose,
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
		return errSDKProtocolEnded
	}
	var protocol *codexsdk.ProtocolError
	if errors.As(err, &protocol) {
		return errors.New("live: protocol request failed")
	}
	if probeDone(err) {
		return errSDKProtocolEnded
	}
	return failedObservation("sdk_observation_failed", "live: SDK observation did not complete")
}

type sdkProductObserver struct {
	ctx          context.Context
	client       *codexsdk.Client
	mu           sync.Mutex
	pending      []frame
	pendingBytes int
	finalize     func() error
	changed      chan struct{}
	clientDone   <-chan struct{}
	runFailures  <-chan error
	cause        error
	onNote       func(frame) error
}

func (transport *sdkProductObserver) setNotification(fn func(frame) error) {
	transport.mu.Lock()
	transport.onNote = fn
	transport.mu.Unlock()
}

func (transport *sdkProductObserver) queued() []frame {
	transport.mu.Lock()
	defer transport.mu.Unlock()
	out := make([]frame, len(transport.pending))
	copy(out, transport.pending)
	return out
}

func (transport *sdkProductObserver) next() (frame, error) {
	for {
		transport.mu.Lock()
		cause, pending, clientDone := transport.cause, len(transport.pending), transport.clientDone
		var message frame
		if cause == nil && pending > 0 {
			message = transport.pending[0]
			transport.pending[0] = frame{}
			transport.pending = transport.pending[1:]
			transport.pendingBytes -= message.size
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
		case err := <-transport.runFailures:
			return frame{}, shippedTransport(transport.ctx, err)
		case <-clientDone:
			return frame{}, errSDKProtocolEnded
		case <-transport.ctx.Done():
			return frame{}, errSDKProtocolEnded
		}
	}
}

func (transport *sdkProductObserver) request(ctx context.Context, _ protocolv2.ServerRequest) (codexsdk.ServerRequestResponse, error) {
	transport.note(ctx, errors.New("live: unsupported server request"))
	return codexsdk.ServerRequestResponse{}, nil
}

func (transport *sdkProductObserver) rawNotification(ctx context.Context, notification codexsdk.RawNotification) error {
	if notification.Method != codexsdk.RawResponseItemCompletedExtension {
		return nil
	}
	raw, err := notification.Envelope.MarshalJSON()
	var envelope struct {
		Params json.RawMessage `json:"params"`
	}
	if err == nil {
		err = json.Unmarshal(raw, &envelope)
	}
	return transport.observeFrame(ctx, frame{Method: notification.Method, Params: envelope.Params, size: len(raw)}, err)
}

func (transport *sdkProductObserver) typedNotification(ctx context.Context, notification protocolv2.ServerNotification) error {
	var method string
	var params any
	switch {
	case notification.Kind() == protocolv2.ServerNotificationKindItemStarted:
		value, _ := notification.AsItemStarted()
		method, params = "item/started", value.Params
	case notification.Kind() == protocolv2.ServerNotificationKindItemCompleted:
		value, _ := notification.AsItemCompleted()
		method, params = "item/completed", value.Params
	case notification.Kind() == protocolv2.ServerNotificationKindTurnStarted:
		value, _ := notification.AsTurnStarted()
		method, params = "turn/started", value.Params
	case notification.Kind() == protocolv2.ServerNotificationKindTurnCompleted:
		value, _ := notification.AsTurnCompleted()
		method, params = "turn/completed", value.Params
	case notification.Kind() == protocolv2.ServerNotificationKindThreadClosed:
		value, _ := notification.AsThreadClosed()
		method, params = "thread/closed", value.Params
	default:
		return nil
	}
	// Serialization measures the existing byte budget only; consumers use SDK values.
	raw, err := json.Marshal(params)
	return transport.observeFrame(ctx, frame{Method: method, Notification: notification, size: len(raw) + len(method) + len(`{"method":"","params":}`)}, err)
}

// The bounded queue belongs to the application observer. SDK admission and
// callback lifecycle remain owned by the public Client/CloseContext contract.
func (transport *sdkProductObserver) observeFrame(ctx context.Context, message frame, marshalErr error) error {
	transport.mu.Lock()
	defer transport.mu.Unlock()
	transport.noteCallback(ctx)
	if marshalErr != nil || message.size == 0 {
		if transport.cause == nil {
			transport.cause = errors.New("live: invalid product completion")
		}
		transport.signal()
		return nil
	}
	if transport.onNote != nil {
		if err := transport.onNote(message); err != nil && transport.cause == nil {
			transport.cause = err
		}
	}
	if len(transport.pending) >= 8192 || transport.pendingBytes+message.size > 128<<20 {
		if transport.cause == nil {
			transport.cause = errors.New("live: product evidence budget exceeded")
		}
	} else {
		transport.pending = append(transport.pending, message)
		transport.pendingBytes += message.size
	}
	transport.signal()
	return nil
}

func (transport *sdkProductObserver) note(ctx context.Context, err error) {
	transport.mu.Lock()
	defer transport.mu.Unlock()
	transport.noteCallback(ctx)
	if transport.cause == nil {
		transport.cause = err
	}
	transport.signal()
}

func (transport *sdkProductObserver) noteCallback(ctx context.Context) {
	if transport.clientDone == nil && ctx != nil {
		transport.clientDone = ctx.Done()
	}
}

func (transport *sdkProductObserver) signal() {
	select {
	case transport.changed <- struct{}{}:
	default:
	}
}

// afterClose only revalidates evidence already credited by the scenario. Public
// CloseContext has joined admitted handlers; cleanup cannot create new proof.
func (transport *sdkProductObserver) afterClose(_ string, evidence *Evidence, cause error) error {
	transport.mu.Lock()
	observed := transport.cause
	transport.mu.Unlock()
	if observed == nil && transport.finalize != nil {
		observed = transport.finalize()
	}
	if cause != nil || observed != nil {
		evidence.Completed, evidence.ChildCompleted, evidence.ChildResultDelivered = false, false, false
	}
	return observed
}
