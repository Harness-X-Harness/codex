#!/usr/bin/env bash
# Prepare only the disposable hosted Ubuntu acceptance runner. The application
# keeps its own filesystem/network sandbox; compilation does not use this step.
set -euo pipefail

if [[ "${GITHUB_ACTIONS:-}" != "true" ||
      "${RUNNER_ENVIRONMENT:-}" != "github-hosted" ||
      "${RUNNER_OS:-}" != "Linux" ]]; then
  echo "Grok sandbox preparation requires a GitHub-hosted Linux Actions job" >&2
  exit 2
fi

runner_os_id="$(. /etc/os-release; printf '%s' "$ID")"
runner_os_version="$(. /etc/os-release; printf '%s' "$VERSION_ID")"
if [[ "$runner_os_id" != "ubuntu" || "$runner_os_version" != "24.04" ]]; then
  echo "Grok sandbox preparation requires the approved Ubuntu 24.04 runner" >&2
  exit 2
fi

# Match the existing stock CI user-namespace prerequisite. Do not silently
# modify additional host settings if that prerequisite changes.
test "$(sysctl -n kernel.unprivileged_userns_clone)" = "1"
sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0
test "$(sysctl -n kernel.apparmor_restrict_unprivileged_userns)" = "0"
