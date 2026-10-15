package live

import (
	"context"
	"errors"
	"reflect"
	"runtime"
	"sync/atomic"
	"unsafe"

	"github.com/ronhuafeng/llm-go/codexsdk"
)

// callbackWorkOffset is the SDK admission counter. The read loop adds before a
// notification is queued, and the dispatcher subtracts only after the handler
// returns. An RPC response is published after that add, on the same read-loop
// goroutine, so a later load of zero happens-after observe.
var callbackWorkOffset = func() uintptr {
	field, ok := reflect.TypeOf(codexsdk.Client{}).FieldByName("callbackWork")
	if !ok || field.Type != reflect.TypeOf(atomic.Int64{}) {
		return ^uintptr(0)
	}
	return field.Offset
}()

func sdkCallbackWork(client *codexsdk.Client) *atomic.Int64 {
	if client == nil || callbackWorkOffset == ^uintptr(0) {
		return nil
	}
	return (*atomic.Int64)(unsafe.Add(unsafe.Pointer(client), callbackWorkOffset))
}

// joinSDKAdmitted waits until handlers admitted before the latest published
// response have returned. The read loop adds callbackWork before it queues a
// notification and only then publishes the response, so a zero load
// happens-after those handlers.
func joinSDKAdmitted(ctx context.Context, client *codexsdk.Client) error {
	work := sdkCallbackWork(client)
	if work == nil {
		return errors.New("live: SDK observation did not complete")
	}
	for work.Load() != 0 {
		if ctx != nil {
			select {
			case <-ctx.Done():
				return errors.New("live: protocol ended before proof completion")
			default:
			}
		}
		runtime.Gosched()
	}
	return nil
}
