# Grok development

Start with [contract- and test-driven development](./docs/carry-forward.md).
That document is the single current engineering policy.

- [Review](./docs/review.md) — inspect behavior, affected stock contracts, tests, and actual CI evidence.
- [Runtime evidence and distribution](./docs/distribution.md) — distinguish backend Facts, product Live, and package execution.
- [Agent entrypoint](../.codex/skills/grok-release-evolution/SKILL.md) — routing to these owners, not another policy.

`grok/main` is a process/documentation overlay, not the Grok product baseline.
A `grok/rust-vX.Y.Z` line may begin at a trusted exact stock tag; changes
then proceed through ordinary, independently testable PRs. Current Stories,
source, tests, and the actual tested Git HEAD define each change's scope and
evidence. Historical C0–C8 milestones and Green Prefix ceremonies are **not**
prerequisites for future development or stock upgrades.

Documentation is not an executable acceptance gate. See the selected version
branch's `grok/TESTING.md` and `grok/docs/stories/README.md` for its real test
commands and supported user-visible contracts.
