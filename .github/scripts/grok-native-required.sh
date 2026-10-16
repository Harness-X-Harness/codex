#!/usr/bin/env bash
# Choose existing native checks; an unavailable comparison always requires them.
set -euo pipefail
if [[ "$GITHUB_EVENT_NAME" == workflow_dispatch ]]; then
  echo true
  exit 0
fi
if [[ "$GITHUB_EVENT_NAME" == pull_request ]]; then
  base="${PR_BASE_SHA:-}"
else
  base="${PUSH_BEFORE:-}"
  if [[ ! "$base" =~ ^[0-9a-f]{40}$ || "$base" == 0000000000000000000000000000000000000000 ]] ||
     ! git cat-file -e "$base^{commit}" 2>/dev/null; then
    echo true
    exit 0
  fi
  # A docs-only followup must not hide earlier unverified native edits on this
  # branch. Compare the entire feature delta, not just the most recent push.
  if ! git check-ref-format "refs/heads/${DEFAULT_BRANCH:-}" >/dev/null 2>&1; then
    echo true
    exit 0
  fi
  if [[ "$GITHUB_REF" == "refs/heads/$DEFAULT_BRANCH" ]]; then
    # There is no independent branch baseline, and no earlier green result is
    # assumed. Preserve applicable evidence even after a docs-only followup.
    echo true
    exit 0
  fi
  if ! base="$(git merge-base "refs/remotes/origin/$DEFAULT_BRANCH" HEAD 2>/dev/null)"; then
    echo true
    exit 0
  fi
fi
if [[ ! "$base" =~ ^[0-9a-f]{40}$ ]] || ! git cat-file -e "$base^{commit}" 2>/dev/null; then
  echo true
  exit 0
fi
printf 'Native comparison base=%s checkout=%s\n' "$base" "$(git rev-parse HEAD)" >&2
files="$(mktemp)"
trap 'rm -f "$files"' EXIT
if ! git diff --name-only --no-renames -z "$base" HEAD > "$files"; then
  echo true
  exit 0
fi
while IFS= read -r -d '' file; do
  case "$file" in
    grok/README.md|grok/TESTING.md|grok/docs/*|grok/package/README.md|grok/package/ARTIFACTS.md) continue ;;
    # These Go-only owners have their own offline/explicit paid-backend tests.
    grok/facts/*|grok/tests/facts/*|grok/tests/live/*|grok/cmd/*|\
    grok/internal/scenarios/*|grok/internal/testresults/*|grok/internal/livecommand/*) continue ;;
    codex-rs/*|sdk/*|scripts/*|grok/*|.github/*|bazel/*|third_party/*|patches/*|tools/*|.cargo/*|\
    .bazel*|MODULE.bazel*|BUILD.bazel|defs.bzl|rbe.bzl|workspace_root_test_launcher.*.tpl|justfile)
      echo true
      exit 0 ;;
  esac
done < "$files"
echo false
