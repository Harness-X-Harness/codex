# Grok stock adoption

This guide owns the Grok-specific maintainer path for moving to a new stock
Codex tag. Runtime semantics stay in [`architecture.md`](./architecture.md),
and delivery semantics stay in [`release.md`](./release.md).

## Common path

```text
choose exact stock rust-vNEW
  -> create grok/rust-vNEW from that tag
  -> replay/adapt the current Grok semantic commits
  -> drop downstream mechanisms stock now owns
  -> open the Codex PR against grok/rust-vNEW
  -> Cargo
  -> push
  -> six binaries + Linux Live
  -> python3 grok/release.py publish
```

Choosing the stock tag and adapting Grok semantics are deliberate product
work.

## Semantic stack

Carry forward current architectural decisions, not the complete history of the
old downstream branch. Each retained commit should represent a Grok semantic
that stock Codex does not yet provide, together with the native tests that
prove that semantic and the stock seam it changes.

The stack is git. On the current line it is

```text
git log --oneline --reverse rust-vOLD..grok/rust-vOLD
```

read oldest first; each commit is one semantic with its tests, and its body
names the seam and the evidence. No document lists the stack, so nothing has
to be kept in step with it.

When new stock Codex already owns a downstream mechanism, drop that mechanism
instead of preserving it for history. Resolve ordinary conflicts according to
the current stock seam and the Grok architecture; a clean cherry-pick is not
acceptance evidence, and a conflict is not by itself an architecture defect.

A fix that is independently correct for stock Codex should be upstreamed when
practical. Until then it may be replayed as a stock-compatible fix rather than
being coupled to Grok-only behavior.

### Fold plan for the next adoption

The 0.154 line accumulated fixes as separate commits so that each could be
proven on its own. At the next adoption they are replayed as the semantic they
belong to, not as history:

| Semantic on `grok/rust-vNEW` | Folds from the 0.154 line |
|---|---|
| project Responses history, reasoning, and tools at the API boundary through a whitelist | the egress half of `147f35ce5`; the fixes `4a65aafb0`, `6dac577a8`, `707c5c322` and their test commits `dd6a73fb0`, `6f5f6003a`; the Stage A whitelist commit and the test commit that deleted the denylist copy. The ingress half of `147f35ce5` (reading Grok responses) stays in this commit or becomes its own if it grows. |
| project tools as flat functions and accept whole-number JSON | `7f72f0825` unchanged; it changes the tool plan and the reverse mapping, not egress |
| prove six-target binaries and Live; publish via `release.py` | `47c1c86ba`, the path filter and inert-head publish `02c7127cf`, `release.py check` `cfa52c31b`, the `gh` color fix `32de65818`, and every later `ci(grok)` commit that only moved workflow steps |
| Live harness (profile, wire recorder, summary) | the `test(grok)` Live harness commits; Stories fold with the harness change that proved them |
| Facts suite | `d481c5300` and its workflow |
| Provider identity and catalog; images through provider policy; App Server binding | `a58e61e66`, `1fdec9205`, `8fee98f33` unchanged unless the stock seam moved |

Docs commits fold into the semantic they document. A `fix(grok)` that stock
now owns is dropped, and its test is kept only if it still asserts a Grok
behavior.

## Procedure

Every step is a command or an existing proof; none needs a new tool.

1. **Choose the tag.** `rust-vNEW` is an exact stock release tag from
   `openai/codex`, fetched by ref:
   `git fetch https://github.com/openai/codex refs/tags/rust-vNEW:refs/tags/rust-vNEW`.
2. **Create the line.** `git checkout -b grok/rust-vNEW rust-vNEW`.
3. **Replay the stack in order.** `git cherry-pick <first>^..<last>` over the
   range from the Semantic stack section, or one semantic at a time when
   applying the fold plan. On conflict, resolve at the current stock seam:
   read what stock now does at that seam and keep the Grok semantic on top
   of it. Never resolve by keeping a mechanism stock now owns, and never by
   merging `rust-vNEW` into the old line.
4. **Recompile is the first decision point.** `cargo check -p codex-api
   -p codex-model-provider -p codex-core` in `codex-rs`. The whitelist
   (`codex-api/src/grok_request.rs`) matches exhaustively over `ResponseItem`
   and tool `type`, so a new stock variant fails to compile there; each
   failure is one decision (emit, omit, or reject, per
   [`request-whitelist.md`](./request-whitelist.md) §"Whitelist mapping for
   Codex") and one native test. A new `ResponsesApiRequest` field does not
   fail the compile; diff `codex-api/src/common.rs` against the old tag and
   decide it the same way. Update the anchors in `request-whitelist.md`.
5. **Run the PR gate locally.** The `cargo` job steps in
   [`grok.yml`](../../.github/workflows/grok.yml) are the list: fmt, clippy
   on the listed crates, the curated `cargo test` steps, the Python unit
   tests, and `go vet && go test` in `grok/live`. Run them from the new line
   before opening the PR.
6. **Open the PR against `grok/rust-vNEW`.** Cargo runs on the PR. Merge by
   fast-forward so the pushed head is the reviewed commit.
7. **Push proof.** Six targets and Linux Live run on the push. RED Live is
   triaged with [`release.md`](./release.md) §Triage; a RED at
   `app_server_start` or `catalog_listed` with `runtime_compatibility` other
   than compatible is the harness (see Live SDK drift), not the product.
8. **Check, then publish.** `python3 grok/release.py check --run-id RUN
   --repo OWNER/NAME`, then `publish` on explicit instruction. The old
   `grok/rust-vOLD` line stops moving because nothing publishes it.

## Live SDK drift

`grok/live` speaks to the App Server through
`github.com/ronhuafeng/llm-go/codexsdk` and its generated `protocolv2`
types. The SDK carries the protocol baseline it was generated from;
`failError` prints it as `generated_baseline=` and the App Server's verdict
as `runtime_compatibility=`.

- When `rust-vNEW` changes the App Server protocol, regenerate or bump the
  SDK first and pin it in `grok/live/go.mod`; the Live harness commit is
  then part of the adoption, before any Story runs.
- A RED whose `runtime_compatibility=` is not compatible, or whose
  `error_type` is an SDK decode error at `app_server_start` or
  `catalog_listed`, is a harness failure: the product did not fail, the
  harness cannot read it. Fix the baseline; do not weaken a Story.
- `go test ./... -count=1` in `grok/live` without `GROK_LIVE` is the
  harness's own unit gate and runs in the Cargo job.

## Trigger

Detecting a new stock tag stays outside the repository: a subscription or
automation on `openai/codex` releases hands the maintainer a tag name. The
procedure starts from that chosen tag. `grok.yml` does not watch upstream and
does not create branches.

## Boundaries

- Do not break the Rules in [`release.md`](./release.md).
- Do not merge a new stock tag into an old Grok branch as the common path.
- Do not replay complete old branch history when current semantic commits are
  sufficient.
- Do not keep a second long-lived product trunk. The Codex tree is `grok/rust-v*`.
- Do not publish from GitHub Actions. Proof stays in `grok.yml`. Channel
  mutation stays in `grok/release.py publish`.
- Do not use Harness branch, CI, or release state as Grok acceptance.
- Do not use Mini as a Grok source or publication coordinator.
- Do not add branch-policy parsers, documentation validators, release ledgers,
  or another migration framework for this procedure.
