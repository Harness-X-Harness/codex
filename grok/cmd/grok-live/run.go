package main

import (
	"context"
	"io"

	"github.com/Harness-X-Harness/codex/grok/internal/livecommand"
)

const usage = livecommand.Usage

func run(ctx context.Context, args []string, getenv func(string) string, output io.Writer) int {
	return livecommand.Run(ctx, args, getenv, output)
}
