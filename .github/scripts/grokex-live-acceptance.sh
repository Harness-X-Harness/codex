#!/usr/bin/env bash
set -euo pipefail
set +x

: "${CODEX_HOME:?CODEX_HOME is required}"
: "${GROKEX_CODEX_BIN:?GROKEX_CODEX_BIN is required}"
: "${GROKEX_WORKSPACE:?GROKEX_WORKSPACE is required}"
: "${GROKEX_OUTPUT_DIR:?GROKEX_OUTPUT_DIR is required}"

mkdir -p "$GROKEX_WORKSPACE" "$GROKEX_OUTPUT_DIR"

python3 .github/scripts/grokex_dual_provider_live.py \
  --grok-only \
  --codex-bin "$GROKEX_CODEX_BIN" \
  --grok-config "$CODEX_HOME/config.toml" \
  --workspace "$GROKEX_WORKSPACE"
