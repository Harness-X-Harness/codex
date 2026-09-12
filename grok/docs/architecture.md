# Grok architecture

Grok adds a third-party Responses API Provider to stock Codex without replacing
the stock harness.

Stock Codex owns session and Thread lifecycle, model selection and Turn
execution, prompts, tools and MCP, Code Mode, Multi-Agent V2 and Ultra,
sandbox and approvals, history and context, and App Server / Codex UI
contracts. Grok-specific code exists only for verified Grok backend API
differences.

This document is the human-readable Grok product design. It is not executable
acceptance input. Deterministic proof is native Cargo tests at the owning
seams. Real-provider composition that those tests cannot prove is owned by the
repository-local Go tests under `grok/live`; that harness consumes
`github.com/ronhuafeng/llm-go/codexsdk` as its App Server SDK dependency. Git
owns source identity as the exact commit SHA.

## Source of truth

Use three current sources with distinct roles:

1. The exact stock Codex tag this line is built from — harness architecture
   and stock behavior.
2. Current Grok source in this repository — implemented Codex behavior and the
   release-bundled Grok catalog.
3. xAI/Grok API behavior — backend protocol semantics.

Working rule: stock Codex owns the harness. Current Provider facts own only
verified backend differences.

Remote Grok Gateway `/models` observations are revalidation input for a future
release. They are not a runtime catalog, cache, fallback, or availability
authority.

## Session and process model

A Codex Thread is bound to one Provider Profile for its lifetime. Model
selection may change only within that Provider through stock behavior.

An App Server process serves one Provider Profile: the `model_provider` it was
started with. Its `model/list` is that Provider's catalog, every Thread it
starts is bound to that Provider, and its resume list is that Provider's
Threads. Using another Provider means starting another process (a
`model_provider` override or a separate `CODEX_HOME`). Catalogs from different
Providers are never merged.

`wire_api = "grok_responses"` is the only serialized selector for the Grok
Provider implementation. The Thread's existing `model_provider` binding
selects the Profile; that Profile constructs one stock Provider instance.

Keep provider identity and model identity separate:

```text
provider = grok
model    = grok-4.6
```

For a Thread bound to the stock OpenAI/ChatGPT Provider, authentication, model
facts, tools, requests, events, durable history, Thread lifecycle, App Server
behavior, and UI behavior remain stock Codex. A process started with the stock
OpenAI/ChatGPT profile is a stock Codex process; Grok adds nothing to its
picker.

## Provider boundary

Codex core operates on canonical Codex concepts. Grok projects at the
narrowest backend boundary:

```text
Codex UI / App Server
        |
        v
Stock Model / Thread contracts
        |
        v
Codex Harness
        |
        v
Stock Provider boundary
        |
        +-- model catalog
        +-- auth / endpoint
        +-- reasoning projection
        +-- request/history projection
        +-- tool wire projection
        +-- response dialect
        |
        v
Grok Responses API
```

A listed projection is the allowed target. Current source and Stories own
whether a projection is implemented.

### Model catalog

The exact release-bundled Grok catalog is the sole runtime catalog authority
for Grok. It contains only models and fields verified for that release. An
explicit config catalog, when supplied through the supported stock seam,
replaces the bundle for that Provider instance; catalogs are never merged.

A new or changed remote model becomes selectable only after a release verifies
and bundles its stock model projection.

### Reasoning projection

Keep logical Codex execution state separate from the backend wire value.
Preserve logical `Ultra` in Codex state and project only the Grok wire effort
to `xhigh` at Provider egress. This does not advertise synthetic Grok
`Ultra` or make an OpenAI-specific internal request valid for Grok.

### Tool projection

Codex builds and routes canonical tools. When the complete path is verified, a
Provider may project a canonical tool identity to a backend-safe wire form and
retain a reversible mapping. Until that round trip is implemented and
verified, Grok namespace-tool capability remains unavailable.

### History projection

Before model input is sent, Grok may project canonical Codex response/history
items into the representation its Responses implementation accepts. Stock
OpenAI remains the identity path. Projection encodes only verified backend
requirements and does not mutate durable history.

### Response decoding and request dialect

Normalize Provider wire responses into stock Codex response items as early as
practical. Wire-level differences (tool declarations, tool-choice, hosted-tool
fields, response-item shapes, streaming events) belong in the Provider/API
boundary. The dialect is internal implementation state, not a second selector.

## Capabilities

Capabilities describe semantics available to Codex after Provider adaptation.
A capability is enabled only after its complete Codex-to-Provider path is
verified. Every other Grok capability remains unavailable.

Any stock internal task that selects a Provider-private model must use
Provider-owned policy. If Grok has no verified model and transport for that
task, fail before Provider egress.

## Ultra and Multi-Agent V2

Multi-Agent V2 remains a stock Codex harness feature. After the complete
history, tool, dialect, capability, and Multi-Agent V2 path is verified for a
Grok model, the target composition is:

```text
Grok Ultra
=
logical Codex Ultra
+ Grok maximum native reasoning effort
+ stock Proactive Multi-Agent V2
```

Reuse the complete stock collaboration lifecycle and controls.

## Stock compatibility

A Thread bound to the stock OpenAI/ChatGPT Provider must remain externally
equivalent to the declared upstream Codex fixed point. Allowed application-level
differences are the selected Provider's stock model projection and Provider
labels in existing App Server and picker fields. Those differences must not
change a ChatGPT-bound Turn's Tool Plan, request admission, transport
lifecycle, durable history, resume/fork/compaction, App Server item shape, or
error behavior.

A change to a seam shared by stock Codex and Grok requires both Grok evidence
and a stock regression at the same observable boundary.

## Upstream adoption

Treat each official upstream Codex tag as the architecture authority for that
candidate. Preserve this design and the verified user-visible outcomes. Do not
preserve a previous Grok implementation shape merely because an earlier stock
version required it.

When stock Codex now owns a required capability, use the stock seam and remove
the superseded Grok mechanism. Port Grok as one semantic commit per stock
seam. Native Grok and stock compatibility tests travel with the behavior they
prove.

The maintainer procedure lives in `grok/docs/carry-forward.md`. Canonical Grok
refs are `grok/<stock-tag>` as described in
[`docs/downstream-products.md`](../../docs/downstream-products.md).

## Proof

```text
native cargo fmt / clippy / test / build
        -> deterministic Grok and stock-seam contracts
direct go test ./... -run '^TestGrok' from grok/live
        -> real Grok composition on the exact Linux archive
Git SHA
        -> source identity
```

Stories in [`stories/`](./stories/) name the user-visible claims and the Rust
or Go test that binds each claim. Do not add documentation validators,
keyword greps, Story inventories as tests, or a second SHA/ledger authority.

Host Goal and independent `/workflow` are a sibling product. Their design is
not this document.

Mini Proxy authorization, grants, credits, routing, credentials, transport,
and accounting are not Grok product contracts. Traffic may pass through Mini;
that does not move those Mini contracts here.
