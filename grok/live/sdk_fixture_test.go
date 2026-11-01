package live

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/Harness-X-Harness/codex/grok/internal/providerfixture"
	"github.com/Harness-X-Harness/codex/grok/internal/testkit"
	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func TestSDKFixtureRetainsInputsUntilPendingCallbackJoins(t *testing.T) {
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	digest, err := testkit.ExecutableSHA256(binary)
	if err != nil {
		t.Fatal(err)
	}
	key, _ := json.Marshal(map[string]string{"Mode": "item", "Trace": filepath.Join(t.TempDir(), "trace")})
	options := Options{
		Subject: Subject{Binary: binary, SHA256: digest, Target: "test-host", Environment: "deterministic", LocalUnverified: true},
		Model:   providerfixture.PrimaryModel, BaseURL: "http://fixture.invalid/v1", APIKey: string(key),
	}
	entered, release := make(chan struct{}), make(chan struct{})
	unblock := sync.OnceFunc(func() { close(release) })
	defer unblock()
	var admitted sync.Once
	var finalized atomic.Bool
	handlers := sdkFixtureHandlers{
		notifications: func(context.Context, protocolv2.ServerNotification) error {
			admitted.Do(func() { close(entered) })
			<-release // Deliberately ignore cancellation to exercise the public pending result.
			return nil
		},
		afterClose: func(string, *Evidence, error) error {
			finalized.Store(true)
			return nil
		},
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	var owned *codexsdk.Client
	var home string
	businessErr := errors.New("controlled business rejection")
	got, err := withSDKFixture(ctx, options, nil, handlers, func(client *codexsdk.Client, cwd string, evidence *Evidence) error {
		owned, home = client, filepath.Dir(cwd)
		thread, err := client.Threads().Start(ctx, protocolv2.ThreadStartParams{Model: protocolv2.Value(options.Model)})
		if err != nil {
			return err
		}
		_, err = client.Turns().Start(ctx, protocolv2.TurnStartParams{ThreadID: thread.Thread.ID,
			Input: []protocolv2.UserInput{protocolv2.NewUserInputText(protocolv2.UserInputText{Text: "controlled"})}})
		if err != nil {
			return err
		}
		select {
		case <-entered:
			return businessErr
		case <-ctx.Done():
			return ctx.Err()
		}
	})
	if !errors.Is(err, businessErr) || !got.CleanupPending || finalized.Load() {
		t.Fatal("unjoined callback was finalized or lost its pending ownership")
	}
	if _, err := os.Stat(home); err != nil {
		t.Fatal("fixture was removed while callback work remained")
	}
	unblock()
	joinCtx, joinCancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer joinCancel()
	for {
		closed, _ := owned.CloseContext(joinCtx)
		if closed.ProcessReaped && closed.CallbackWorkPending == 0 {
			break
		}
		if joinCtx.Err() != nil {
			t.Fatal("released controlled callback failed to join")
		}
		time.Sleep(time.Millisecond)
	}
	_ = os.RemoveAll(home)
}
