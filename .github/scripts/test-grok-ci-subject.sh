#!/usr/bin/env bash
# Offline Git fixtures for event identity and affected-check fail-closed behavior.
set -euo pipefail
scripts="$(cd "$(dirname "$0")" && pwd)"
root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT
cd "$root"
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
export GIT_AUTHOR_NAME=fixture GIT_AUTHOR_EMAIL=fixture@example.invalid
export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME" GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"
git init -q --initial-branch=feature
mkdir -p .github/workflows grok/docs grok/internal/testresults codex-rs
printf 'initial\n' > .github/workflows/grok-ci.yml
git add .
git commit -qm initial
base="$(git rev-parse HEAD)"
git update-ref refs/remotes/origin/main "$base"
export DEFAULT_BRANCH=main
export GITHUB_REPOSITORY=owner/repo GITHUB_RUN_ID=123 GITHUB_RUN_ATTEMPT=1
export GITHUB_EVENT_NAME=push GITHUB_REF=refs/heads/feature
export GITHUB_SHA="$base" GITHUB_WORKFLOW_SHA="$base" PUSH_AFTER="$base" PUSH_BEFORE="$base"
export GITHUB_WORKFLOW_REF="$GITHUB_REPOSITORY/.github/workflows/grok-ci.yml@$GITHUB_REF"
subject() { bash "$scripts/check-grok-subject.sh"; }
native() { bash "$scripts/grok-native-required.sh"; }
reject() {
  if "$@" >/dev/null 2>&1; then
    echo 'Invalid CI subject was accepted' >&2
    exit 1
  fi
}
subject | grep -q '"GROK_SUBJECT_KIND": "branch_head"'
test "$(native)" = false
(export GITHUB_REF=refs/heads/main; test "$(native)" = true)
(export GITHUB_EVENT_NAME=workflow_dispatch; subject >/dev/null; test "$(native)" = true)
(export GITHUB_SHA=0000000000000000000000000000000000000000; reject subject)
(export PUSH_AFTER=0000000000000000000000000000000000000000; reject subject)
(export GITHUB_WORKFLOW_SHA=0000000000000000000000000000000000000000; reject subject)
(export GITHUB_WORKFLOW_REF="$GITHUB_REPOSITORY/.github/workflows/unreviewed.yml@$GITHUB_REF"; reject subject)
(export GITHUB_REF=refs/tags/feature; export GITHUB_WORKFLOW_REF="$GITHUB_REPOSITORY/.github/workflows/grok-ci.yml@$GITHUB_REF"; reject subject)
(export GITHUB_EVENT_NAME=pull_request_target; reject subject)
(export PUSH_BEFORE=0000000000000000000000000000000000000000; test "$(native)" = true)
(export PUSH_BEFORE=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; test "$(native)" = true)

# Each native build input must trigger PR/main events and applicability.
# Independent branches avoid credit from a previous native edit in this loop.
index=0
for entry in 'bazel/build.bzl:bazel/**' 'third_party/source.cc:third_party/**' \
  'patches/fix.patch:patches/**' 'tools/build-tool:tools/**' \
  '.cargo/config.toml:.cargo/**' 'rbe.bzl:rbe.bzl' \
  'workspace_root_test_launcher.sh.tpl:workspace_root_test_launcher.*.tpl' \
  'workspace_root_test_launcher.bat.tpl:workspace_root_test_launcher.*.tpl'; do
  file="${entry%%:*}"
  pattern="${entry#*:}"
  test "$(grep -Fc -- "- \"$pattern\"" "$scripts/../workflows/grok-ci.yml")" -eq 2
  index=$((index + 1))
  git checkout -qb "native-input-$index" "$base"
  mkdir -p "$(dirname "$file")"
  printf 'native input\n' > "$file"
  git add . && git commit -qm native-input
  test "$(native)" = true
  git checkout -q feature
done

printf 'docs\n' > grok/docs/change.md
git add . && git commit -qm docs
test "$(native)" = false
printf 'package testresults\n' > grok/internal/testresults/change.go
git add . && git commit -qm go
test "$(native)" = false
printf 'product prompt\n' > codex-rs/prompt.md
git add . && git commit -qm prompt
test "$(native)" = true
printf 'fn main() {}\n' > codex-rs/change.rs
git add . && git commit -qm rust
test "$(native)" = true
# A later docs-only push still requires the branch's native edits.
export PUSH_BEFORE="$(git rev-parse HEAD)"
printf 'later docs\n' >> grok/docs/change.md
git add . && git commit -qm later-docs
test "$(native)" = true
(export DEFAULT_BRANCH=missing; test "$(native)" = true)
printf 'changed\n' >> .github/workflows/grok-ci.yml
git add . && git commit -qm workflow
export GITHUB_SHA="$(git rev-parse HEAD)" PUSH_AFTER="$(git rev-parse HEAD)"
reject subject
export GITHUB_WORKFLOW_SHA="$GITHUB_SHA"
subject >/dev/null

# A real two-parent fixture: a merge result cannot be mislabeled as source HEAD.
git checkout -qb base "$base"
printf 'base\n' > base.txt
git add . && git commit -qm base
export PR_BASE_SHA="$(git rev-parse HEAD)"
export PR_HEAD_SHA="$GITHUB_SHA"
git merge -q --no-ff feature -m merge
export GITHUB_EVENT_NAME=pull_request PR_NUMBER=7 GITHUB_REF=refs/pull/7/merge
export GITHUB_SHA="$(git rev-parse HEAD)" GITHUB_WORKFLOW_SHA="$(git rev-parse HEAD)"
export GITHUB_WORKFLOW_REF="$GITHUB_REPOSITORY/.github/workflows/grok-ci.yml@$GITHUB_REF"
subject | grep -q '"GROK_SUBJECT_KIND": "pr_test_merge"'
test "$(native)" = true
(export PR_HEAD_SHA="$PR_BASE_SHA"; reject subject)
(export PR_BASE_SHA="$PR_HEAD_SHA"; reject subject)
(export GITHUB_SHA="$PR_HEAD_SHA"; reject subject)
printf 'Grok CI subject and applicability fixtures passed\n'
