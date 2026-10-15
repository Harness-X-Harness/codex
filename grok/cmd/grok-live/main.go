// Command grok-live runs one existing explicit-binary scenario without retry.
package main

import (
	"context"
	"os"
	"os/signal"

	"github.com/Harness-X-Harness/codex/grok/live"
)

func main() {
	if handled, code := live.ServeStdoutFilter(os.Args[1:]); handled {
		os.Exit(code)
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	code := run(ctx, os.Args[1:], os.Getenv, os.Stdout)
	stop()
	os.Exit(code)
}
