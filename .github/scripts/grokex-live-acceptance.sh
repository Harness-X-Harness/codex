#!/usr/bin/env bash
set -euo pipefail
set +x

: "${CODEX_HOME:?CODEX_HOME is required}"
: "${GROKEX_CODEX_BIN:?GROKEX_CODEX_BIN is required}"
: "${GROKEX_WORKSPACE:?GROKEX_WORKSPACE is required}"
: "${GROKEX_OUTPUT_DIR:?GROKEX_OUTPUT_DIR is required}"

mkdir -p "$GROKEX_WORKSPACE" "$GROKEX_OUTPUT_DIR"

report_failure_class() {
  local jsonl="$1"
  local stderr_file="$2"
  local exit_status="$3"
  local message
  local event_count
  local event_types
  local json_status

  if jq -e 'type == "object"' "$jsonl" >/dev/null 2>&1; then
    json_status=valid
    event_count="$(jq -s 'length' "$jsonl")"
    event_types="$(
      jq -r '
        .type? // empty
        | if test("^[A-Za-z0-9._/-]{1,80}$") then . else "other" end
      ' "$jsonl" \
        | LC_ALL=C sort -u \
        | paste -sd, -
    )"
  elif [[ -s "$jsonl" ]]; then
    json_status=invalid
    event_count=unknown
    event_types=unknown
  else
    json_status=empty
    event_count=0
    event_types=none
  fi
  if [[ -z "$event_types" ]]; then
    event_types=none
  fi
  echo "exit_status=${exit_status}" >&2
  echo "json_status=${json_status}" >&2
  echo "json_event_count=${event_count}" >&2
  echo "json_event_types=${event_types}" >&2
  if [[ -s "$stderr_file" ]]; then
    echo "stderr_status=nonempty" >&2
  else
    echo "stderr_status=empty" >&2
  fi

  message="$(
    jq -r '
      select(.type == "turn.failed" or .type == "error")
      | if .type == "turn.failed" then .error.message else .message end
    ' "$jsonl" 2>/dev/null | tail -n 1
  )"

  case "$message" in
    *"401"*|*"unauthorized"*|*"authentication"*) echo "failure_class=authentication" >&2 ;;
    *"403"*|*"forbidden"*) echo "failure_class=authorization" >&2 ;;
    *"404"*|*"not found"*) echo "failure_class=endpoint_or_model_not_found" >&2 ;;
    *"422"*|*"invalid"*schema*|*"invalid request"*) echo "failure_class=provider_contract" >&2 ;;
    *"429"*|*"rate limit"*) echo "failure_class=rate_limit" >&2 ;;
    *"timed out"*|*"timeout"*) echo "failure_class=timeout" >&2 ;;
    *"model"*catalog*|*"model"*provider*) echo "failure_class=provider_catalog" >&2 ;;
    "")
      if grep -Eqi '401|unauthorized|authentication' "$stderr_file"; then
        echo "failure_class=authentication" >&2
      elif grep -Eqi '403|forbidden' "$stderr_file"; then
        echo "failure_class=authorization" >&2
      elif grep -Eqi '404|not found' "$stderr_file"; then
        echo "failure_class=endpoint_or_model_not_found" >&2
      elif grep -Eqi '422|invalid.*schema|invalid request' "$stderr_file"; then
        echo "failure_class=provider_contract" >&2
      elif grep -Eqi '429|rate limit' "$stderr_file"; then
        echo "failure_class=rate_limit" >&2
      elif grep -Eqi 'timed out|timeout' "$stderr_file"; then
        echo "failure_class=timeout" >&2
      elif grep -Eqi 'config.*(parse|load|invalid)|toml|configuration' "$stderr_file"; then
        echo "failure_class=configuration" >&2
      elif grep -Eqi 'environment variable|env var|api key.*(missing|required|set)' "$stderr_file"; then
        echo "failure_class=credential_environment" >&2
      elif grep -Eqi 'model.*catalog|model.*provider|provider.*model' "$stderr_file"; then
        echo "failure_class=provider_catalog" >&2
      elif grep -Eqi 'no such file|not found.*(binary|executable)|cannot execute|exec format' "$stderr_file"; then
        echo "failure_class=binary_runtime" >&2
      elif grep -Eqi 'panic|panicked|segmentation fault|aborted' "$stderr_file"; then
        echo "failure_class=client_runtime" >&2
      else
        echo "failure_class=unknown" >&2
      fi
      ;;
    *) echo "failure_class=provider_or_runtime" >&2 ;;
  esac
}

run_exec() {
  local name="$1"
  local exit_status
  shift
  exit_status=0
  "$GROKEX_CODEX_BIN" exec \
    --json \
    --dangerously-bypass-approvals-and-sandbox \
    --skip-git-repo-check \
    -C "$GROKEX_WORKSPACE" \
    "$@" \
    >"$GROKEX_OUTPUT_DIR/${name}.jsonl" \
    2>"$GROKEX_OUTPUT_DIR/${name}.stderr" || exit_status=$?
  if ((exit_status != 0)); then
    echo "Grokex live acceptance case failed: ${name}" >&2
    report_failure_class \
      "$GROKEX_OUTPUT_DIR/${name}.jsonl" \
      "$GROKEX_OUTPUT_DIR/${name}.stderr" \
      "$exit_status"
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

run_exec subagent \
  "Use spawn_agent to create one reviewer. Tell it to reply with exactly GROKEX_SUBAGENT_CHILD_OK. Wait for it, then return exactly that marker."
require_output 'GROKEX_SUBAGENT_CHILD_OK' \
  "$GROKEX_OUTPUT_DIR/subagent.jsonl" "sub-agent result"
require_rollout '"type":"agent_message"' "canonical sub-agent history"

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
