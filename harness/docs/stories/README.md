# Harness Stories

User-visible Harness composition claims. Deterministic proof is
`goal-host-checks`. The unique Live Story below is required when a candidate
changes Thread/Turn lifecycle, extension composition, Goal HOW, Workflow
auto-resume, or engine ownership.

These files are not executable acceptance input.

| Story | Proof |
|-------|-------|
| [Host-owned goal stays distinct from an independent workflow](./host-goal-remains-distinct-from-independent-workflow.md) | `goal-host-checks` plus the Live Then on a real Thread |

Architecture: [`../architecture.md`](../architecture.md).
