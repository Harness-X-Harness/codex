#!/usr/bin/env bash
# Validate Actions' event-owned subject; callers must not substitute a source SHA.
set -euo pipefail

checkout_sha="$(git rev-parse HEAD)"
[[ "$GITHUB_SHA" =~ ^[0-9a-f]{40}$ && "$GITHUB_WORKFLOW_SHA" =~ ^[0-9a-f]{40}$ ]]
test "$checkout_sha" = "$GITHUB_SHA"
git cat-file -e "$GITHUB_WORKFLOW_SHA^{commit}"
case "$GITHUB_WORKFLOW_REF" in
  "$GITHUB_REPOSITORY/.github/workflows/grok.yml@$GITHUB_REF"|\
  "$GITHUB_REPOSITORY/.github/workflows/grok-ci.yml@$GITHUB_REF") ;;
  *) echo 'Unexpected Grok workflow identity' >&2; exit 1 ;;
esac
case "$GITHUB_EVENT_NAME" in
  pull_request)
    [[ "${PR_NUMBER:-}" =~ ^[0-9]+$ ]]
    [[ "${PR_BASE_SHA:-}" =~ ^[0-9a-f]{40}$ && "${PR_HEAD_SHA:-}" =~ ^[0-9a-f]{40}$ ]]
    test "$GITHUB_REF" = "refs/pull/$PR_NUMBER/merge"
    test "$(git rev-parse HEAD^1)" = "$PR_BASE_SHA"
    test "$(git rev-parse HEAD^2)" = "$PR_HEAD_SHA"
    subject_kind=pr_test_merge
    ;;
  push|workflow_dispatch)
    [[ "$GITHUB_REF" == refs/heads/* ]]
    git check-ref-format "$GITHUB_REF"
    # For push, cross-check the webhook subject as well as the checkout.
    if [[ "$GITHUB_EVENT_NAME" == push ]]; then test "${PUSH_AFTER:-}" = "$GITHUB_SHA"; fi
    subject_kind=branch_head
    ;;
  *) echo 'Unexpected Grok execution event' >&2; exit 1 ;;
esac
# Preserve the reviewed caller, reusable workflow and local-action boundary.
# This compares definitions, without treating a caller-supplied SHA as authority.
git diff --exit-code "$GITHUB_WORKFLOW_SHA" HEAD -- .github
export GROK_CHECKOUT_SHA="$checkout_sha" GROK_SUBJECT_KIND="$subject_kind"
python3 - <<'PY'
import json, os
fields = ("GITHUB_REPOSITORY", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_EVENT_NAME",
          "GITHUB_REF", "GITHUB_SHA", "GITHUB_WORKFLOW_REF", "GITHUB_WORKFLOW_SHA",
          "PR_HEAD_SHA", "PR_BASE_SHA", "GROK_CHECKOUT_SHA", "GROK_SUBJECT_KIND")
print(json.dumps({key: os.environ.get(key, "") for key in fields}, sort_keys=True))
PY
