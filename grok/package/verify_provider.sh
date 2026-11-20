#!/usr/bin/env bash
# Current harness acceptance of a verified package, with a controlled backend only.
set -euo pipefail
cd "$(dirname "$0")/.."
evidence="$1"
mkdir -p "$evidence"
export GROK_FACTS=0 GROK_LIVE=0 CC=cc CXX=c++
status=0
go test -race -tags=integration -count=1 -timeout=8m -json ./tests/integration \
  -run '^Test(BasicFixture|ShippedCatalog|StructuredEditFixture|ShippedChildFixture|FirstRequestShape)$' \
  > "$evidence/provider-fixture.json" || status=$?
go run ./cmd/grok-test-results --input "$evidence/provider-fixture.json" \
  --integration-owner TestBasicFixture \
  --integration-owner TestShippedCatalog \
  --integration-owner TestStructuredEditFixture \
  --integration-owner TestShippedChildFixture \
  --integration-owner TestFirstRequestShape \
  > "$evidence/provider-fixture-summary.json" || status=1
cat "$evidence/provider-fixture-summary.json"
exit "$status"
