#!/usr/bin/env bash
# Download the release archives built for one product tree.
#
#   grokex/download-archives.sh <product_tree> <output_dir> <target>...
#
# Each target's archive lives in the newest non-expired artifact named
# grokex-build-<product_tree>-<target>, whichever workflow run uploaded it.
# Requires GH_TOKEN with actions:read on the current repository.
set -euo pipefail

product_tree="$1"
output="$2"
shift 2
(( $# > 0 )) || { echo "no targets requested" >&2; exit 2; }

mkdir -p "${output}"
for target in "$@"; do
  artifact="grokex-build-${product_tree}-${target}"
  run_id="$(gh api "repos/${GITHUB_REPOSITORY}/actions/artifacts?name=${artifact}&per_page=20" \
    --jq '[.artifacts[] | select(.expired == false)] | sort_by(.created_at) | reverse | .[0].workflow_run.id // empty')"
  if [[ -z "${run_id}" ]]; then
    echo "no archive artifact for ${artifact}" >&2
    exit 1
  fi
  echo "${artifact} <- run ${run_id}"
  gh run download "${run_id}" --repo "${GITHUB_REPOSITORY}" --name "${artifact}" --dir "${output}"
done
ls -l "${output}"
