# Grok request whitelist

Strategy and plan for constructing Grok Responses egress from a whitelist at
the existing `ResponsesDialect::project_request` seam, replacing the denylist
that serializes the OpenAI-shaped request and removes fields afterwards.

This is a maintainer design record. It is not executable acceptance input.
Runtime semantics stay in [`architecture.md`](./architecture.md), delivery in
[`release.md`](./release.md), and stock-tag adoption in
[`carry-forward.md`](./carry-forward.md).

## Revision anchors

Revise this document when any anchor moves. The tables below describe these
exact sources, not "current" Codex or grok-build.

| Anchor | Value |
|--------|-------|
| Stock Codex | `rust-v0.154.0` (`6b9826e3a`) |
| Grok source | `grok/rust-v0.154.0` @ `707c5c322` (2026-09-16) |
| Seam | `codex-rs/codex-api/src/provider.rs` `ResponsesDialect::project_request`, called only from `codex-rs/codex-api/src/endpoint/responses.rs` `stream_request` |
| grok-build | [`xai-org/grok-build`](https://github.com/xai-org/grok-build) `main` @ `4827113` (2026-09-15) |
| grok-build request constructor | `crates/codegen/xai-grok-sampling-types/src/conversation/responses.rs` (blob `abe5cda`) |
| grok-build hosted-tool entries | `crates/codegen/xai-grok-sampling-types/src/tool_overrides.rs` (blob `2abea09`) |
| grok-build conversation model | `crates/codegen/xai-grok-sampling-types/src/conversation.rs` (blob `83e88f9`) |
| grok-build binding tests | `crates/codegen/xai-grok-sampling-types/src/conversation/responses_tests.rs` (blob `d297def`) |

grok-build is xAI's own harness. Its typed request constructor is the best
available statement of what the Grok Responses backend consumes. It is an
authority for *shape*; the Live run on the Codex artifact is the authority for
*acceptance*.

## Philosophy

```text
Same seam. Construct, do not subtract. Evidence before emission.
```

1. **Same seam.** Grok differences are projected once, at the API boundary,
   on a request copy. Stock OpenAI stays the identity path. There is no
   `if grok` in `spec_plan`, `hosted_spec`, `client.rs`, protocol models,
   rollout, or App Server. Durable history is never rewritten.
2. **Construct, do not subtract.** The Grok request is built from the
   canonical `ResponsesApiRequest` into Grok-only wire types and then
   serialized. A field reaches Grok because a type declares it, not because
   nothing removed it. Serializing the OpenAI request and deleting keys is the
   denylist this document retires.
3. **Evidence before emission.** A field is in the whitelist for one of two
   reasons, each recorded next to it: a Codex transport requirement (for
   example `stream`, `include`) or a Grok semantic (for example
   `encrypted_content`, `call_id`). Evidence order: a GREEN Live run on the
   Codex artifact, then grok-build's typed model, then xAI documentation. An
   observed xAI `400` is evidence to remove or reshape; it is never patched
   around by adding another removal.
4. **Fail closed, decide at compile time.** The mapping over `ResponseItem`
   is an exhaustive `match` without a wildcard arm. A new stock variant stops
   compiling until a maintainer decides: map it, drop it as a request control,
   or reject it before transport. Semantic-bearing items Grok cannot receive
   are rejected before transport, never silently dropped.
5. **Unknowns are probed, not guessed.** Neither denylist expansion nor
   whitelist tightening happens without an observed rejection or a GREEN
   Live run. Speculative removals are listed as open probes, not encoded as
   facts.

Denylist fails open: each new stock field leaks until a user reports a `400`.
Whitelist fails closed: a new stock semantic is not sent until a maintainer
maps it, and the compiler names the place. The second failure is cheaper and
visible in review.

## Why the current implementation is a patch

`project_request` for `ResponsesDialect::Grok` today:

1. clones the request;
2. maps plaintext `AgentMessage` and named unpaired `FunctionCallOutput` to
   user messages, sets reasoning `content` to `Some([])` when a blob exists
   so stock serde omits the key;
3. rejects encrypted collaboration history and orphan outputs;
4. `serde_json::to_value` of the OpenAI-shaped request;
5. rewrites `web_search` to `{"type":"web_search"}`, appends `x_search`,
   drops the tool-control trio when there are no tools;
6. recursively removes `external_web_access`, `indexed_web_access`,
   `defer_loading` anywhere in the JSON;
7. drops `compaction_trigger` items, removes `namespace`, removes reasoning
   `content`/`encrypted_content` per blob state, removes `status` and
   `encrypted_function_args` from call items.

Inventory of the removals and what motivated each:

| Removal | Evidence | Class |
|---------|----------|-------|
| `external_web_access`, `indexed_web_access` | Codex app `{"code":"400","error":"Argument not supported: external_web_access"}` | observed rejection |
| reasoning `content` omitted when a non-empty blob exists; `content: null` and empty blob omitted | Codex app `{"code":"invalid-argument","error":"Could not decode the compaction blob..."}` on the second Turn; removing the `content` key restored `200` | observed rejection |
| bare `web_search`, appended `x_search` | Live GREEN | product rule |
| tool-control trio omitted with no tools | Live-verified no-tool request | product rule |
| `compaction_trigger` dropped | stock request control; Grok `remote_compaction` is `Unsupported` | structural |
| `encrypted_function_args` removed | already cleared by stock `client.rs` for every non-OpenAI provider | redundant |
| `namespace` removed | flat projection already names tools; no observed `400` | speculative |
| `status` removed from `custom_tool_call`, `tool_search_call`, `web_search_call`, `image_generation_call` | no observed `400`; `image_generation_call` **with** `status` was Live GREEN at `c4c80eef`; grok-build strips `status` only on reasoning input and replays hosted calls as-is | speculative |
| `defer_loading` removed | stock deferred-tool extra; no observed `400` | speculative |

Two rounds of production `400`s in the 0.154 line came from fields stock
0.154 serializes that stock 0.153 did not. The third round was a speculative
sweep. Each round is a new removal in the same function, and the next stock
tag will add fields this function has never seen.

## grok-build reference model

What `impl From<&ConversationRequest> for rs::CreateResponse` constructs:

| Field | Value |
|-------|-------|
| `model`, `temperature`, `top_p`, `max_output_tokens` | from the request |
| `prompt_cache_key` | request key, else `x_grok_conv_id` |
| `reasoning` | `Some({ effort, summary })` |
| `tool_choice` | mapped when set |
| `tools` | function tools only; `None` when empty |
| `text` | only a strict `json_schema` format |
| `input` | `build_responses_input`, see below |

Explicitly `None` in that constructor: `background`, `conversation`,
`include`, `instructions`, `max_tool_calls`, `metadata`,
`parallel_tool_calls`, `previous_response_id`, `prompt`,
`prompt_cache_retention`, `safety_identifier`, `service_tier`, `store`,
`stream`, `stream_options`, `truncation`. This is their sampler's shape, not a
list of fields Grok rejects. Their transport sets streaming elsewhere, and
they never ask for `include` because their sampler consumes encrypted
reasoning from the response body directly.

Input items (`conversation_item_to_input_items`):

| Conversation item | Emitted |
|-------------------|---------|
| System / User | `{ type: message, role, content }` |
| Reasoning | typed sibling, cloned with `status = None`; keeps `id`, `summary`, `encrypted_content`, `content` |
| Assistant text | `{ type: message, role: assistant, content }` |
| Assistant tool call | `function_call { call_id, name, arguments }`, `id: None`, `status: None` |
| Tool result | `function_call_output { call_id, output }`, `id: None`, `status: None` |
| Backend tool call | `web_search_call`, `custom_tool_call` (x_search), `code_interpreter_call` replayed as-is |

`patch_reasoning_text_types` adds `type: reasoning_text` to reasoning
`content` entries when they exist because async-openai omits the
discriminator and the API answers `400`. Their reasoning tests
(`test_encrypted_reasoning_included_in_responses_api_request`,
`test_only_encrypted_reasoning_included_in_request`) all replay
`content: None` with a blob. grok-build therefore does **not** prove that
well-typed `content` and `encrypted_content` may coexist; Codex evidence says
they may not.

Tools (`build_responses_tools`, `extra_tool_entries`, `to_tool_entry`):

| Tool | Emitted |
|------|---------|
| function | `{ type: function, name, description, parameters }`, `strict: None` |
| function colliding with a hosted name | dropped; hosted wins |
| hosted `web_search` | `{ type: web_search }` or `{ type, filters: { allowed_domains } }` / `{ ..., excluded_domains }`; lists mutually exclusive, max 5 |
| hosted `x_search` | `{ type: x_search }` or with `from_date` / `to_date` |

Their model has no `external_web_access`, `indexed_web_access`,
`search_context_size`, `user_location`, `search_content_types`,
`defer_loading`, `namespace`, or `encrypted_function_args`.

## Whitelist mapping for Codex

Decision values: **emit** (in the whitelist with evidence), **omit** (not
constructed), **reject** (error before transport), **probe** (Stage B; keep
today's egress until Live decides).

### Request fields

| `ResponsesApiRequest` field | Decision | Evidence / reason |
|-----------------------------|----------|-------------------|
| `model` | emit | Live |
| `instructions` | emit | Live GREEN; grok-build places the system prompt in `input`, which is equivalent |
| `input` | emit (mapped) | below |
| `tools` | emit when non-empty; omit `tools`, `tool_choice`, `parallel_tool_calls` together when empty | Live-verified no-tool request |
| `tool_choice` (`"auto"`) | emit with tools | Live |
| `parallel_tool_calls` | probe | Live GREEN today; grok-build `None` |
| `reasoning.effort`, `reasoning.summary` | emit | grok-build; Live |
| `reasoning.context` | omit | stock sets it only for `use_responses_lite`, which the Grok catalog disables |
| `store: false` | probe | Live GREEN today; grok-build `None` |
| `stream: true` | emit | Codex SSE transport requirement |
| `stream_options` | omit | stock sets it only for OpenAI |
| `include: ["reasoning.encrypted_content"]` | emit | encrypted-reasoning continuation Story; Codex must receive blobs to replay them |
| `service_tier` | omit | OpenAI tiering |
| `prompt_cache_key` | emit | grok-build; Live |
| `text` | emit only `format: json_schema`; omit verbosity | grok-build; Grok catalog does not support verbosity |
| `client_metadata` | probe | Live GREEN today; Codex-backend telemetry with no Grok function |
| `access_programs` | omit | OpenAI cyber access program |

### Input items

Pre-mapping (Codex-only concepts grok-build does not have) stays on the
request copy exactly as today:

- plaintext `AgentMessage` becomes a user `message`;
- named unpaired `FunctionCallOutput` becomes a user `message`;
- encrypted `AgentMessage` content is **rejected** before transport;
- `FunctionCallOutput` without `call_id` is **rejected** before transport.

| `ResponseItem` | Grok item | Fields | Evidence / reason |
|----------------|-----------|--------|-------------------|
| `Message` | `message` | `id?`, `role`, `content[]` (`input_text`, `input_image{image_url,detail?}`, `output_text`) | Live; `phase` omitted; passthrough metadata already cleared by stock for non-OpenAI |
| `Reasoning` | `reasoning` | `id?`, `summary[]`, `encrypted_content` when non-empty; `content` only when there is no usable blob and it is well-typed | observed rejection for `content` + blob and for `content: null`; grok-build strips `status` only |
| `FunctionCall` | `function_call` | `call_id`, `name`, `arguments`, `id?` | grok-build; Live. `namespace` and `encrypted_function_args` are not constructed |
| `FunctionCallOutput` | `function_call_output` | `call_id`, `output` (text or content items) | grok-build; Live |
| `CustomToolCall` | `custom_tool_call` | `call_id`, `name`, `input`, `id?` | custom `apply_patch` Story; `status` is a probe |
| `CustomToolCallOutput` | `custom_tool_call_output` | `call_id`, `output` | custom `apply_patch` Story |
| `WebSearchCall` | `web_search_call` | `id?`, `action?`; `status` probe | grok-build replays as-is with status |
| `ImageGenerationCall` | `image_generation_call` | `id?`, `status`, `revised_prompt?`, `result` | image-edit Story was GREEN with `status` at `c4c80eef`; dropping it is a probe |
| `CompactionTrigger` | dropped | — | stock request control; Grok `remote_compaction` `Unsupported` |
| `ConfigurationUpdate` | reject | — | stock records it only for OpenAI + `use_responses_lite`; cannot appear on a Grok Thread |
| `Compaction`, `ContextCompaction` | reject | — | produced only by remote compaction, which Grok does not have |
| `LocalShellCall` | reject | — | OpenAI `local_shell` tool; Grok offers `exec_command` functions |
| `ToolSearchCall`, `ToolSearchOutput`, `AdditionalTools` | reject | — | OpenAI tool-search surface; not advertised to Grok |
| `Other` | dropped | — | unknown inbound item; nothing to replay |

A rejection is a bug report at the right seam: it means stock produced an
item for a Grok Thread that the Grok product line has not decided how to
send. It replaces a remote `400` with a local error naming the item.

### Tools

`ResponsesApiTools` is pre-serialized JSON. The whitelist parses it into
values, maps each by `type`, and constructs Grok tool types.

| Codex tool JSON | Grok tool | Evidence / reason |
|-----------------|-----------|-------------------|
| `function { name, description, parameters, strict, defer_loading? }` | `function { name, description, parameters }` | grok-build `strict: None`; `strict` stays emitted as a probe until Live decides; `defer_loading` is not constructed |
| `custom { name, description, format }` | `custom` as-is | custom `apply_patch` Story |
| `web_search { external_web_access, indexed_web_access, filters, user_location, search_context_size, search_content_types }` | `web_search { filters: { allowed_domains }? }` | grok-build `to_tool_entry`; Codex config exposes only `allowed_domains`; today filters are dropped, restoring them is Stage B |
| `x_search` | appended once when tools are non-empty | Grok capability rule, Live GREEN; grok-build emits it only when the hosted tool is requested and Codex has no `x_search` config |
| `namespace`, `tool_search` | reject | flat projection already flattens namespaces; reaching the whitelist is a flat-projection regression |
| any other `type` | reject | undecided tool surface |

### Divergences from grok-build kept on purpose

| Field | grok-build | Codex whitelist | Reason |
|-------|------------|-----------------|--------|
| `include` | `None` | `["reasoning.encrypted_content"]` | Codex replays blobs from its own history |
| `stream` | `None` | `true` | Codex SSE client |
| `instructions` | `None` (system in input) | emitted | stock prompt assembly; Live GREEN |
| `id` on call items | `None` | passed through when present | stock strips non-prefixed ids upstream; Live GREEN |
| `x_search` | on request only | always with tools | Grok product capability |
| reasoning `content` with blob | patched type, kept | omitted | observed Codex rejection; grok-build never sends both |

### Open probes (Stage B)

Each probe is one commit with a GREEN Live run or an observed rejection as
its evidence. Until then the whitelist emits today's egress.

| Probe | Question | How to decide |
|-------|----------|---------------|
| function `strict` | does Grok accept or ignore `strict: true`? | drop it; Live GREEN on the custom `apply_patch` and dynamic-tool Stories |
| `web_search.filters.allowed_domains` | does Grok accept Codex's `allowed_domains`? | emit filters from `web_search` config; Live with a filtered search |
| `status` on `custom_tool_call`, `web_search_call`, `image_generation_call` | required, ignored, or rejected on input? | replay with and without; image-edit Story covers `image_generation_call` |
| `parallel_tool_calls`, `store`, `client_metadata` | ignored or consumed? | drop one per commit; Live GREEN |
| reasoning `content` with blob | is a well-typed `reasoning_text` channel rejected, or only `null`? | one Live Turn N+1 with `[{type: reasoning_text, text}]` + blob; keep omission if `400` |

## Module plan

Same seam, new module. `provider.rs` stays the selector; the constructor
moves out so `provider.rs` shrinks instead of growing.

```text
codex-rs/codex-api/src/provider.rs
  ResponsesDialect::for_provider        unchanged
  ResponsesDialect::project_request     OpenAi => identity serde
                                        Grok   => grok_request::build(request)
  strip_unsupported_grok_arguments      deleted
  JSON retain/remove post-processing    deleted

codex-rs/codex-api/src/grok_request.rs            new, target < 500 LoC
  pub(crate) fn build(&ResponsesApiRequest) -> Result<Value, GrokProjectionError>
  GrokResponsesRequest                  Serialize only
  GrokInputItem                         #[serde(tag = "type")], exhaustive from ResponseItem
  GrokContentItem, GrokReasoningItem, GrokFunctionCallOutput
  GrokTool                              function | custom | web_search | x_search
  GrokWebSearchFilters                  allowed_domains?
  GrokProjectionError                   rejected item / tool, mapped to serde::ser::Error at the seam

codex-rs/codex-api/src/grok_request_tests.rs      Grok dialect tests move here
```

Rules for the module:

- Types derive `Serialize` only. Nothing deserializes from Grok here; inbound
  decoding stays in `sse/responses.rs`.
- `match` over `ResponseItem` and over tool `type` is exhaustive with no
  wildcard arm.
- No `include_str!` or file reads, so `BUILD.bazel` needs no `compile_data`.
- No changes to `spec_plan.rs`, `hosted_spec.rs`, `client.rs`, protocol
  models, rollout, `responses_websocket.rs` (Grok `supports_websockets`
  is `false`), or `compact.rs` (Grok `remote_compaction` is `Unsupported`).

## Staging

### Stage A: whitelist layer, behavior-preserving

One semantic commit:

```text
feat(grok): construct Grok Responses egress from a whitelist
```

Contract: for every request fixture in the existing Grok dialect tests and
for one fixture per accepted `ResponseItem` variant and tool type,
`grok_request::build(request)` equals the JSON the current denylist produces.
The golden comparison is a test in `grok_request_tests.rs` that runs the old
projection kept only in the test module until the commit lands; the old
projection is then deleted with the commit. Stage A changes no bytes on the
wire, so the six-target build and the Linux musl Live must be GREEN with
identical user-visible outcomes.

Stage A also adds the compile-time decision point (exhaustive match) and the
reject-before-transport errors for items and tools that cannot appear on a
Grok Thread. Those are the only behavior differences, and each is a local
error where today the request would reach xAI.

### Stage B: tighten toward grok-build, one probe per commit

Each open probe above is one commit with a native test and a GREEN Live run.
A probe that fails stays in this table with the observed error text.

### Delivery

PR to `grok/rust-v*` runs Cargo. Push runs six targets and Live. Publish
follows [`release.md`](./release.md): only from a GREEN run whose SHA is the
branch head. A docs-only or whitelist commit that lands after a GREEN run
moves the head and needs its own GREEN run before `grok/release.py publish`.

## Tests

- `codex-rs/codex-api/src/grok_request_tests.rs`: dialect identity
  (`for_provider`), copy-only canonical request, golden equivalence per
  fixture, one test per `ResponseItem` variant and tool type, rejections,
  and the stock OpenAI identity control (`stock_openai_projection_remains_identity`).
- `codex-rs/core/tests/suite/grok_web_search.rs` and
  `codex-rs/core/tests/suite/grok_reasoning_replay.rs`: keep asserting the
  outbound `/responses` body through the full core path.
- Live: `grok/live` `go test -run '^TestGrok'` on the musl binary. The
  encrypted-reasoning continuation, image-edit, and custom `apply_patch`
  Stories exercise the reasoning, hosted-replay, and custom-tool rows above.

Prefer `pretty_assertions::assert_eq` on whole projected bodies over
per-key assertions.

## Semantic stack placement

The 0.154 line at `707c5c322`:

```text
a58e61e66 feat(grok): add Provider identity and bundled catalog
147f35ce5 feat(grok): project Responses history and reasoning at the API boundary
7f72f0825 feat(grok): project tools as flat functions and accept whole-number JSON
1fdec9205 feat(grok): advertise and generate images through provider policy
8fee98f33 feat(grok): bind App Server lifecycle to the Grok Provider
47c1c86ba ci(grok): prove six-target binaries and Live; publish via release.py
4a65aafb0 fix(grok): drop unsupported external_web_access on Responses egress
dd6a73fb0 test(grok): drop unused ModelProvider import
6dac577a8 fix(grok): omit reasoning content when replaying encrypted blobs
6f5f6003a test(grok): drop follow-up reasoning id assertion
707c5c322 fix(grok): drop OpenAI-only history and deferred-tool extras
```

On this line the whitelist layer is an additional semantic commit after the
fixes. At the next stock adoption it becomes the request-projection semantic
itself: the egress half of `147f35ce5` and the five fix/test commits fold into
one commit, "project Responses history, reasoning, and tools at the API
boundary through a whitelist". The flat-tool projection (`7f72f0825`) stays
separate; it changes the tool plan and the reverse mapping, not egress.

Estimated size: new module and tests around 500 to 700 changed lines,
`provider.rs` shrinks by about 150. Within the 800-line review guidance.

## Revision procedure

**Stock Codex moves.** Re-run the compile: a new `ResponseItem` variant or
tool `type` fails the exhaustive match; a new `ResponsesApiRequest` field is
invisible to the constructor until mapped. Decide each with evidence and
update the mapping tables. Update the stock anchor.

**grok-build moves.** Diff the four anchored files against the recorded blob
SHAs. A new field in `CreateResponse`, `conversation_item_to_input_items`, or
`to_tool_entry` is a candidate row; a removed field is a candidate probe.
Update the grok-build anchors.

**xAI returns a new rejection.** Record the exact error text and the request
shape in the test name. The fix is a change to a Grok type or a mapping row,
never a new key removal on serialized JSON.

## Non-goals

- `if grok` in `spec_plan`, `hosted_spec`, or any harness module.
- Rewriting durable history or inventing history projections.
- Turning the field problem into configuration (`web_search = "disabled"`,
  capability flags) at the cost of hosted search.
- Touching `CompactClient` or `RemoteCompactionV2`; the compaction-blob `400`
  was reasoning replay, not a compact RPC.
- Making the official stock Codex app speak Grok; it has no dialect and users
  replace the binary with the published Grok artifact.
- A second selector. `wire_api = "grok_responses"` and the resolved Provider
  identity remain the only dialect selectors.
