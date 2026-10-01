# Provider foundation Stories

These are stable user-visible contracts for #320's opt-in Live scenario source.
The shared deterministic proof tests the harness mechanics; it does not execute
or prove these Stories. Facts observe the backend separately and cannot complete
a product Story. Exact execution subjects and results belong in GitHub.

| Story | Scenario source | Boundary |
| --- | --- | --- |
| [Configured Grok fixture completes a text turn](./grok-provider-fixture-turn.md) | Basic fixture | Selected binary's App Server v2 |
| [Encrypted reasoning survives same-thread continuation](./grok-encrypted-reasoning-history-continuation.md) | Primary and pinned reasoning fixtures | Selected binary's App Server v2 |

The fixture models are controls inherited from the previous accepted reference,
not shipped defaults or catalog policy. #329 owns shipped profile/catalog checks;
#322 owns the tool semantics used by the reasoning scenario. #320 introduces
the consumed harness and scenario source. #331 owns actual Live execution and
#339 owns package-to-Live invocation. No backend execution is claimed here.
