package live

import (
	"context"
	"encoding/json"
	"reflect"
	"testing"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func TestProductSDKRequestHandlerRefusesUnrequestedTools(t *testing.T) {
	observer := &sdkProductObserver{ctx: context.Background(), changed: make(chan struct{}, 1)}
	// Requests enter only the SDK request handler, never the notification queue.
	request := protocolv2.NewServerRequestItemToolCall(protocolv2.ServerRequestItemToolCall{
		Params: protocolv2.DynamicToolCallParams{ThreadID: "private-thread", TurnID: "private-turn", CallID: "private-call", Tool: "write_file"},
	})
	response, err := observer.request(context.Background(), request)
	if err != nil || !reflect.DeepEqual(response, codexsdk.ServerRequestResponse{}) {
		t.Fatal("unsupported request returned an authorized tool response")
	}
	if len(observer.queued()) != 0 {
		t.Fatal("request became notification evidence")
	}
	if _, err := observer.next(); err == nil || err.Error() != "live: unsupported server request" {
		t.Fatal("request did not poison product completion")
	}
	evidence := Evidence{Completed: true, ChildCompleted: true, ChildResultDelivered: true}
	if observer.afterClose("", &evidence, nil) == nil || evidence.Completed || evidence.ChildCompleted || evidence.ChildResultDelivered {
		t.Fatal("request refusal allowed completed product evidence")
	}
}

func TestProductSDKNotificationRetainsTypedEvidenceAndByteBudget(t *testing.T) {
	observer := &sdkProductObserver{ctx: context.Background(), changed: make(chan struct{}, 1)}
	notification := protocolv2.NewServerNotificationTurnCompleted(protocolv2.ServerNotificationTurnCompleted{
		Params: protocolv2.TurnCompletedNotification{ThreadID: "thread", Turn: protocolv2.Turn{ID: "turn", Status: protocolv2.TurnStatusCompleted, Items: []protocolv2.ThreadItem{}}},
	})
	if err := observer.typedNotification(context.Background(), notification); err != nil {
		t.Fatal(err)
	}
	pending := observer.queued()
	if len(pending) != 1 || len(pending[0].Params) != 0 || !reflect.DeepEqual(pending[0].Notification, notification) {
		t.Fatal("typed notification was replaced by a private JSON projection")
	}
	value, _ := notification.AsTurnCompleted()
	raw, err := json.Marshal(value.Params)
	if err != nil {
		t.Fatal(err)
	}
	size := len(raw) + len("turn/completed") + len(`{"method":"","params":}`)
	if pending[0].size != size || observer.pendingBytes != size {
		t.Fatal("typed evidence changed byte accounting")
	}
	if _, err := observer.next(); err != nil || observer.pendingBytes != 0 {
		t.Fatal("dequeue lost byte accounting")
	}
	observer.pendingBytes = 128<<20 - size + 1
	if err := observer.typedNotification(context.Background(), notification); err != nil {
		t.Fatal(err)
	}
	if observer.cause == nil || len(observer.queued()) != 0 {
		t.Fatal("oversized typed evidence was retained")
	}
}
