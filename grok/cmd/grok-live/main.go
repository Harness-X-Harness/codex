// Command grok-live runs one existing explicit-binary scenario without retry.
package main

import (
	"context"
	"os"
	"os/signal"
)

func main() {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	code := run(ctx, os.Args[1:], os.Getenv, os.Stdout)
	stop()
	os.Exit(code)
}
