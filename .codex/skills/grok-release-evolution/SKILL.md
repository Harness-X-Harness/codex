---
name: grok-release-evolution
description: Route Grok stock-version adoption, ordinary contract- and test-driven development, review, runtime evidence, and optional distribution to the current grok/main owners. Not for unrelated stock Codex work.
---

# Grok development entrypoint

Read `grok/main:grok/docs/carry-forward.md` as the single current development
policy. Read `review.md` in that directory for review, and
`distribution.md` for real-backend evidence, packages or publication. This
skill routes to those owners.

Resolve the current target/source HEAD, Story/Issue, code/test owner and
applicable CI before acting. For stock upgrades, pin old stock, formal old
Grok HEAD and new stock; first follow the complete-net-delta temporary commit
and cherry-pick procedure in `carry-forward.md`, then adapt and verify.
The selected product branch's `grok/TESTING.md` and
`grok/docs/stories/README.md` own local test commands and current consumer
claims.

For each requested change, identify its observable contract, affected stock
and product behavior, direct technical dependencies, and nearest executable
tests. Prefer a focused failing test for a new regression, and preserve
material positive/negative tests through refactoring. Keep owned generated
outputs and consumers with the implementation; use actual Rust/Go/native/
Integration/paid-backend tiers according to impact.

Apply the acceptance and evidence rules in `carry-forward.md`. Identify the
actual tested source and distinguish it from a PR merge subject, runtime
binary and optional artifact/backend evidence. Use `review.md` to inspect
the applicable checks and configured repository rules.

Use genuine technical prerequisites or independently testable outcomes to
split work.
When tests or Actions are incomplete, report exact run/attempt/subject and
unresolved evidence rather than assigning an acceptance status.

Read/review does not authorize mutation. Confirm the user's requested
write scope and account permissions; never infer permission to reset a
branch, force-push, edit protection, publish a release, or expose secrets.
Verify any external write through authoritative readback and report only
what actually changed.
