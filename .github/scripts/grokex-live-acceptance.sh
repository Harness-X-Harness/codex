#!/usr/bin/env bash
set -euo pipefail
set +x

: "${CODEX_HOME:?CODEX_HOME is required}"
: "${GROKEX_CODEX_BIN:?GROKEX_CODEX_BIN is required}"
: "${GROKEX_WORKSPACE:?GROKEX_WORKSPACE is required}"
: "${GROKEX_OUTPUT_DIR:?GROKEX_OUTPUT_DIR is required}"

mkdir -p "$GROKEX_WORKSPACE" "$GROKEX_OUTPUT_DIR"

run_exec() {
  local name="$1"
  shift
  if ! "$GROKEX_CODEX_BIN" exec \
    --json \
    --dangerously-bypass-approvals-and-sandbox \
    --skip-git-repo-check \
    -C "$GROKEX_WORKSPACE" \
    "$@" \
    >"$GROKEX_OUTPUT_DIR/${name}.jsonl" \
    2>"$GROKEX_OUTPUT_DIR/${name}.stderr"; then
    echo "Grokex live acceptance case failed: ${name}" >&2
    return 1
  fi
}

require_output() {
  local pattern="$1"
  local file="$2"
  local label="$3"
  if ! grep -Eq "$pattern" "$file"; then
    echo "Grokex live acceptance evidence is missing: ${label}" >&2
    return 1
  fi
}

require_rollout() {
  local pattern="$1"
  local label="$2"
  if ! grep -R -E -q "$pattern" "$CODEX_HOME/sessions"; then
    echo "Grokex durable rollout evidence is missing: ${label}" >&2
    return 1
  fi
}

run_exec local \
  "Use the local shell tool to run printf GROKEX_LOCAL_OK. Return only that marker."
require_output 'GROKEX_LOCAL_OK' "$GROKEX_OUTPUT_DIR/local.jsonl" "local tool result"

thread_id="$(
  jq -r 'select(.type == "thread.started") | .thread_id' \
    "$GROKEX_OUTPUT_DIR/local.jsonl" | head -n 1
)"
if [[ -z "$thread_id" || "$thread_id" == "null" ]]; then
  echo "Grokex live acceptance did not emit a thread id." >&2
  exit 1
fi

run_exec resume resume "$thread_id" \
  "Use the local shell tool to run printf GROKEX_RESUME_OK. Return only that marker."
require_output 'GROKEX_RESUME_OK' "$GROKEX_OUTPUT_DIR/resume.jsonl" "resume result"

run_exec fork fork "$thread_id" \
  "Use the local shell tool to run printf GROKEX_FORK_OK. Return only that marker."
require_output 'GROKEX_FORK_OK' "$GROKEX_OUTPUT_DIR/fork.jsonl" "fork result"

run_exec web \
  "Use web search to find the official xAI home page. Return its domain only."
require_rollout '"type":"web_search_call"' "native web search item"

run_exec x_search \
  "Use X search to find a recent public post from the official xAI account. State the account name only."
require_rollout '"type":"custom_tool_call".*"name":"x_(keyword|semantic)_search"' "native X search item"

run_exec image \
  "Use image generation to create a simple blue circle on a white background."
require_rollout '"type":"(grok_)?image_generation_call"' "native image generation item"

echo "Grokex live acceptance passed."
