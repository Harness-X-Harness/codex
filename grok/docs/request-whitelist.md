# Grok request whitelist

Strategy and plan for constructing Grok Responses egress from a whitelist at
the existing `ResponsesDialect::project_request` seam, replacing the denylist
that serializes the OpenAI-shaped request and removes fields afterwards, and
for surfacing Grok backend abilities through the same Provider boundary.

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
| Grok source | `grok/rust-v0.154.0` @ `439c47384` (2026-09-16) |
| Seam | `codex-rs/codex-api/src/provider.rs` `ResponsesDialect::project_request` (OpenAi identity; Grok → `grok_request::build`), called only from `codex-rs/codex-api/src/endpoint/responses.rs` `stream_request` |
| grok-build | [`xai-org/grok-build`](https://github.com/xai-org/grok-build) `main` @ `4827113` (2026-09-15) |
| grok-build request constructor | `crates/codegen/xai-grok-sampling-types/src/conversation/responses.rs` (blob `abe5cda`) |
| grok-build hosted-tool entries | `crates/codegen/xai-grok-sampling-types/src/tool_overrides.rs` (blob `2abea09`) |
| grok-build conversation model | `crates/codegen/xai-grok-sampling-types/src/conversation.rs` (blob `83e88f9`) |
| grok-build binding tests | `crates/codegen/xai-grok-sampling-types/src/conversation/responses_tests.rs` (blob `d297def`) |
| Backend facts | `grok/facts` `TestFact*`, recorded 2026-09-16 against `grok.trustedtunnel.app/v1` (Mini proxy to xAI) |

grok-build is xAI's own harness. Its typed request constructor is the best
available statement of what the Grok Responses backend consumes. It is an
authority for *shape*; the Live run on the Codex artifact is the authority for
*acceptance*.

## Direction

```text
Keep the Codex harness. Hide what Grok cannot take. Surface what Grok can do.
```

The product goal has two sides. Codex harness abilities stay available on a
Grok Thread through stock behavior or a stock alternative. Grok backend
abilities are surfaced through the Provider boundary. OpenAI-specific
features that Grok cannot accept, even after a shape transform, are hidden
before the harness plans them.

The set the egress constructor emits is therefore:

```text
(Codex canonical  ∩  Grok accepts)  ∪  Grok-native extensions
```

not a subset of the OpenAI request. Grok-native extensions enter through
stock configuration and Provider seams and have their own rows in the
mapping tables; they are not hard-coded at egress.

Three layers share this work. Each has one responsibility.

| Layer | Responsibility | Where |
|-------|----------------|-------|
| Capability | **Hide.** Decide what the harness plans for a Grok Thread, so unsupported OpenAI features are never produced | Grok catalog `ModelInfo` flags, `ProviderCapabilities`, `ModelProviderInfo`, stock `is_openai` gates |
| Egress whitelist | **Transform and extend.** Construct the Grok request from what the harness produced; emit Grok-native fields | `ResponsesDialect::project_request` |
| Ingress dialect | **Recognize.** Decode Grok wire items into stock response items; mark backend-executed calls so the harness does not dispatch them | `sse/responses.rs`, `grok_stream.rs`, `ModelProvider::is_provider_hosted_tool_call` |

### Ingress

Streaming-event differences are sequenced at this layer before stock
`process_responses_event`. Stock OpenAI is the identity path.

| Difference | Grok wire | Adaptation | Tests |
|------------|-----------|------------|-------|
| Stream order | Grok interleaves output items (message stays open across hosted calls); stock consumes one active item | `grok_stream.rs` sequences frames by `output_index`; stock OpenAI is identity | `grok_stream_tests.rs`, `grok_hosted_stream.rs`, `sse/responses.rs` OpenAI identity |
| Hosted `custom_tool_call` | Grok x_search calls are backend-complete; stock prompt history requires a client `custom_tool_call_output` | `for_prompt_with_hosted_calls` skips pairing when `is_provider_hosted_tool_call` is true; no synthetic output | `history_tests.rs`, `grok_hosted_x_search.rs` |

A rejection at egress is the guard for the capability layer, not its
implementation. When the whitelist rejects an item, the fix is normally a
capability-layer change that stops the harness producing it for Grok.

### Capability layer today

Hiding already happens at stock seams. These are the facts the egress
whitelist relies on; a rejection row below is reachable only if one of them
regresses.

| Hidden OpenAI feature | Stock seam | Grok value | Harness ability kept through |
|-----------------------|------------|------------|------------------------------|
| `tool_search`, deferred tool loading | `ModelInfo.supports_search_tool` | `false` (`grok_catalog.rs`) | full tool list in the plan |
| namespace tool wire form | `ProviderCapabilities.namespace_tools` + `projects_tools_as_flat_functions` | `true` + `true` | stock namespaces planned, flattened at egress, restored on the reverse map |
| `local_shell` tool | `ModelInfo.shell_type` | `UnifiedExec` | `exec_command` functions |
| OpenAI `apply_patch` function | `ModelInfo.apply_patch_tool_type` | `Freeform` | custom `apply_patch` |
| verbosity `text.verbosity` | `ModelInfo.support_verbosity` | `false` | — |
| `reasoning.summary` parameter | `ModelInfo.supports_reasoning_summary_parameter` | `false` | reasoning summaries stay off |
| Responses Lite, `reasoning.context`, `configuration_update` | `ModelInfo.use_responses_lite` + stock `is_openai` gate | `false` | request-level `reasoning.effort` each Turn |
| remote compaction, `compaction_trigger`, `compaction` items | `ProviderCapabilities.remote_compaction` | `Unsupported` | stock local compaction |
| Responses over WebSocket | `ModelProviderInfo.supports_websockets` | `false` | HTTP SSE |
| `stream_options`, `encrypted_function_args`, passthrough metadata | stock `client.rs` `is_openai` gate | non-OpenAI | — |
| standalone `/alpha/search` | `Feature::StandaloneWebSearch` | default off (`Stage::UnderDevelopment`); Grok has no such route | hosted `web_search` |
| Guardian and other Codex-backend routes | stock `uses_codex_backend` requires `is_openai` | false for Grok | Provider-owned preferred-model policy (`approval_review_preferred_model`, memory models) |

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
   nothing removed it. The types cover the intersection with Grok plus the
   Grok-native extensions above; they are not a copy of the OpenAI types.
   Serializing the OpenAI request and deleting keys is the denylist this
   document retires.
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
constructed), **reject** (error before transport), **probe** (Stage B1 or
B2; keep today's egress until Live decides).

### Request fields

| `ResponsesApiRequest` field | Decision | Evidence / reason |
|-----------------------------|----------|-------------------|
| `model` | emit | Live |
| `instructions` | emit | Live GREEN; grok-build places the system prompt in `input`, which is equivalent |
| `input` | emit (mapped) | below |
| `tools` | emit when non-empty; omit `tools` and `tool_choice` together when empty | Live-verified no-tool request |
| `tool_choice` (`"auto"`) | emit with tools | Live |
| `parallel_tool_calls` | omit | `TestFactParallelToolCallsStoreClientMetadata` (`accepted` for `parallel_tool_calls: false`) ≠ consumed; grok-build `None`; this B1 probe |
| `reasoning.effort`, `reasoning.summary` | emit | grok-build; Live |
| `reasoning.context` | omit | stock sets it only for `use_responses_lite`, which the Grok catalog disables |
| `store: false` | omit | `TestFactParallelToolCallsStoreClientMetadata` (`accepted` for `store: false`) ≠ consumed; grok-build `None`; this B1 probe |
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
| `CustomToolCall` | `custom_tool_call` | `id`, `call_id`, `name`, `input`, `status?` | on a Grok Thread this is only a replayed backend-executed x_search call (client custom tools are flattened to `function_call`); grok-build replays it as-is. Live (`TestGrokHostedXSearch`). `id` is required on replay (`TestFactCustomToolCallReplayRequiresID`: `422 missing field id`); `status` is accepted (`TestFactInputStatusOnHostedItems`) |
| `CustomToolCallOutput` | `custom_tool_call_output` | `call_id`, `output` | custom `apply_patch` Story |
| `WebSearchCall` | `web_search_call` | `id?`, `action` | Live (`TestGrokHostedWebSearch`): Turn 2 replays `id` + `action` and Grok accepts it. grok-build replays as-is with status; `status` is accepted (`TestFactInputStatusOnHostedItems`) but not constructed. `action` is required on replay (`TestFactWebSearchCallReplayRequiresAction`: `422 missing field action`) |
| `ImageGenerationCall` | `image_generation_call` | `id?`, `status`, `revised_prompt?`, `result` | image-edit Story was GREEN with `status` at `c4c80eef`; `status` is accepted (`TestFactInputStatusOnHostedItems`) |
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
| `function { name, description, parameters, strict, defer_loading? }` | `function { name, description, parameters }` | omit `strict`: grok-build `strict: None`; `TestFactFunctionStrict` (`accepted`) is not consumed; this B1 probe. `defer_loading` is not constructed |
| `custom { name, description, format }` | `custom` as-is | custom `apply_patch` Story |
| `web_search { external_web_access, indexed_web_access, filters, user_location, search_context_size, search_content_types }` | `web_search { filters: { allowed_domains }? }` | grok-build `to_tool_entry`; Codex config exposes only `allowed_domains`; today filters are dropped, restoring them is B2 |
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

## Grok-native capability surface

Grok abilities that grok-build models and that the Codex Provider boundary
can carry. Each row names the stock entry the ability must use, the egress
row that emits it, the ingress handling that recognizes its result, and the
current status. `Implemented` means code and native Cargo tests exist;
`Live` means a named Story proves it on the artifact.

| Ability | grok-build shape | Codex entry | Egress | Ingress | Status |
|---------|------------------|-------------|--------|---------|--------|
| hosted `web_search` | `{type: web_search}` | `web_search` config / `WebSearchMode` | bare `web_search` | stock `web_search_call` | Live (`TestGrokHostedWebSearch`): a real hosted Turn records `web_search_call`, every message delta reaches the client, and Turn 2 replays it |
| `web_search` domain allowlist | `filters.allowed_domains` (max 5) | stock `web_search.filters.allowed_domains` | today dropped by the bare rewrite | — | Not surfaced (B2) |
| `web_search` domain blocklist | `filters.excluded_domains` (max 5, exclusive with allowlist) | none; stock config has no `excluded_domains` | — | — | Not surfaced; needs a config seam first (B2) |
| hosted `x_search` | `{type: x_search}` | none; Grok product rule appends it with any tools | `x_search` appended | `is_provider_hosted_tool_call` marks completed `custom_tool_call` named `x_keyword_search`, `x_semantic_search`, `x_user_search`, `x_thread_fetch` so the harness records instead of dispatching | Live (`TestGrokHostedXSearch`); predicate has a native test |
| `x_search` date window | `from_date` / `to_date` (`YYYY-MM-DD`) | none | — | — | Not surfaced (B2) |
| encrypted reasoning continuation | reasoning sibling with `encrypted_content` | stock `include` | `reasoning` row | stock `reasoning` item | Live (`TestGrokEncryptedReasoningContinuation`) |
| image generation and history edit | — (Codex-specific hosted item) | provider policy (`ProviderCapabilities.image_generation`) | `image_generation_call` replay | stock | Live (`TestGrokImageGenerationEdit`) |
| custom `apply_patch` | `custom` tool | `ModelInfo.apply_patch_tool_type = Freeform` | `custom` tool as-is | flat `function_call` reverse map | Live (`TestGrokCustomApplyPatch`) |
| maximum native reasoning effort | `reasoning.effort` | catalog reasoning projection (`Ultra` → `xhigh`) | `reasoning.effort` | — | Implemented |
| prompt-cache routing | `prompt_cache_key` | stock | emitted | — | Live |
| hosted `code_interpreter` | `code_interpreter_call` replay | none | — | none; Codex has no response item, so it would land in `Other` | Not surfaced; not advertised, so never emitted by Grok |

Ingress note: grok-build treats **every** `custom_tool_call` in a Grok
response as a backend-executed call. Codex enumerates four names. If Grok
adds an x_search sub-tool, the harness would dispatch it as a client tool
and answer with an error output. Aligning to "any completed
`custom_tool_call` on a Grok Thread is backend-executed" is a B2 candidate
with a Live probe; client custom tools cannot collide because they are
flattened to `function_call` for Grok.

### Open probes

Each probe is one commit with a GREEN Live run or an observed rejection as
its evidence. Until then the whitelist emits today's egress. Each B1/B2
probe names its `TestFact*`; a Facts run is the evidence for the backend
class. Rejected classes are `rejected:<HTTP status>/<error>` with the
backend `error` text whitespace-collapsed and cut at 160 characters.

P0, decides whether a Grok-native ability already shipped works end to end:

| Probe | Question | How to decide |
|-------|----------|---------------|
| real x_search Turn | does a Grok Turn that invokes x_search complete, record the hosted call, and continue on the next Turn? | one Live Story with a prompt that requires X content; assert a completed hosted `custom_tool_call` in the session and a terminal reply; assert Turn N+1 replays it without a `400`. Live (`TestGrokHostedXSearch`): Turn 1 records a completed hosted `custom_tool_call`; Turn 2 replays it and Grok accepts it (pairing skip: #249). |

Recorded facts (backend class, independent of the binary; `grok/facts`,
first recorded 2026-09-16 against `grok.trustedtunnel.app`). A Facts run is
the evidence; a flip revises the row, not a publish.

| Fact | Request shape | Recorded class |
|------|---------------|----------------|
| `TestFactWebSearchExternalWebAccessRejected` | `tools: [{type: web_search, external_web_access: true}]` | `rejected:400/Argument not supported: external_web_access` |
| `TestFactReasoningNullContentWithBlobRejected` | reasoning `content: null` + blob | `rejected:400/Could not decode the compaction blob. Ensure it is unmodified from the compact response.` |
| `TestFactReasoningTypedContentWithBlob` | reasoning `content: [{type: reasoning_text, text}]` + blob | `accepted` |
| `TestFactIncludeEncryptedReasoning` | `include: ["reasoning.encrypted_content"]` | `accepted`; a reasoning item with non-empty `encrypted_content` is returned |
| `TestFactFunctionStrict` | function tool with `strict: true` | `accepted` |
| `TestFactInputStatusOnHostedItems` | `status: completed` on replayed `custom_tool_call`, `web_search_call`, `image_generation_call` | `accepted` for all three |
| `TestFactCustomToolCallReplayRequiresID` | replayed `custom_tool_call` without `id` | `rejected:422/… invalid "custom_tool_call" item: missing field \`id\`` |
| `TestFactWebSearchCallReplayRequiresAction` | replayed `web_search_call` without `action` | `rejected:422/… invalid "web_search_call" item: missing field \`action\`` |
| `TestFactParallelToolCallsStoreClientMetadata` | each of `parallel_tool_calls: false`, `store: false`, `client_metadata` alone | `accepted` for each |
| `TestFactWebSearchAllowedDomains` | `tools: [{type: web_search, filters: {allowed_domains: [...]}}]` | `accepted` |
| `TestFactXSearchDateWindow` | `tools: [{type: x_search, from_date, to_date}]` | `accepted` |
| `TestFactTextVerbosityRejectedOrIgnored` | `text: {verbosity: low}` | `accepted`; whitelist omits it until an effect is observed |

What the recorded facts settle for the whitelist:

- Input items: `custom_tool_call.id` and `web_search_call.action` are
  required on replay. The whitelist must carry both from history; dropping
  either is a `422`, not a silent ignore.
- Reasoning: only `content: null` next to a blob is rejected. A typed
  `reasoning_text` channel is accepted, so B1 may emit it when stock records
  one; omission stays the conservative default.
- Function tool `strict` is omitted (`TestFactFunctionStrict` `accepted` ≠
  consumed; grok-build `strict: None`). `parallel_tool_calls` is omitted
  (`TestFactParallelToolCallsStoreClientMetadata` `accepted` for
  `parallel_tool_calls: false` ≠ consumed; grok-build `None`). `store` is
  omitted by this B1 probe (`TestFactParallelToolCallsStoreClientMetadata`
  `accepted` for `store: false` ≠ consumed; grok-build `None`). Remaining
  accepted probes are `client_metadata`, `text.verbosity`: B1 drops
  each remaining one per commit and watches Live; none can be a `400`
  source today.
- `web_search.filters.allowed_domains` and the `x_search` date window are
  accepted, so B2 needs only the stock-compatible config seam, not a backend
  probe.

B1, tighten toward grok-build:

| Probe | Question | How to decide | Fact |
|-------|----------|---------------|------|
| function `strict` | omit (decided) | `feat(grok): omit function tool strict on Grok Responses egress`; Live GREEN on the custom `apply_patch` and dynamic-tool Stories is the post-merge line proof | `TestFactFunctionStrict` (`accepted`) |
| `status` on `custom_tool_call`, `web_search_call`, `image_generation_call` | keep or drop on replay? | accepted either way; keep what stock records | `TestFactInputStatusOnHostedItems` (`accepted`) |
| `parallel_tool_calls` | omit (decided) | `feat(grok): omit parallel_tool_calls on Grok Responses egress`; Live GREEN is the post-merge line proof | `TestFactParallelToolCallsStoreClientMetadata` (`accepted` for `parallel_tool_calls: false`) |
| `store` | omit (decided) | this commit (`feat(grok): omit store on Grok Responses egress`); Live GREEN is the post-merge line proof | `TestFactParallelToolCallsStoreClientMetadata` (`accepted` for `store: false`) |
| `client_metadata` | consumed or only accepted? | drop one per commit; Live GREEN | `TestFactParallelToolCallsStoreClientMetadata` (`accepted`) |
| reasoning `content` with blob | emit the typed channel or keep omitting? | one Live Turn N+1 with `[{type: reasoning_text, text}]` + blob; keep omission unless a Story needs the text | `TestFactReasoningTypedContentWithBlob` (`accepted`) |

B2, extend with Grok-native abilities:

| Probe | Question | How to decide | Fact |
|-------|----------|---------------|------|
| `web_search.filters.allowed_domains` | which stock config seam carries it? | emit filters from the stock `web_search` config instead of the bare rewrite; Live with a filtered search | `TestFactWebSearchAllowedDomains` (`accepted`) |
| `web_search.filters.excluded_domains` | which stock-compatible config seam carries a blocklist? | add the config field through the stock `web_search` config path, validate exclusivity and the cap of 5, then emit; Live | — |
| `x_search` date window | which config seam carries `from_date` / `to_date`? | Grok Provider config, validated `YYYY-MM-DD`; emit on the `x_search` entry; Live | `TestFactXSearchDateWindow` (`accepted`) |
| any completed `custom_tool_call` is hosted | can `is_provider_hosted_tool_call` drop the name list? | widen the predicate; run the P0 Story and the custom `apply_patch` Story | — |

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
- Ingress recognition stays in `codex-rs/model-provider/src/grok_provider.rs`
  (`is_provider_hosted_tool_call`) and `sse/responses.rs`. B2 changes there
  travel with their own tests; the egress module does not decode responses.

## Staging

### Stage A: whitelist layer, behavior-preserving

Landed by this commit: `feat(grok): construct Grok Responses egress from a whitelist` replaces the denylist in `project_request` with `grok_request::build`.

One semantic commit:

```text
feat(grok): construct Grok Responses egress from a whitelist
```

Contract: for every request fixture in the existing Grok dialect tests and
for one fixture per accepted `ResponseItem` variant and tool type,
`grok_request::build(request)` equals the JSON the current denylist produces.
The golden comparison was a test in `grok_request_tests.rs` that ran the old
projection, kept only in the test module while the commit landed; the
following commit deleted that copy and kept the fixtures as
`accepted_fixtures`, which every accepted item and tool type must build from
without an OpenAI-only key. Stage A changes no bytes on the wire, so the
six-target build and the Linux musl Live must be GREEN with identical
user-visible outcomes.

Stage A also adds the compile-time decision point (exhaustive match) and the
reject-before-transport errors for items and tools that cannot appear on a
Grok Thread. Those are the only behavior differences, and each is a local
error where today the request would reach xAI.

### Stage B1: tighten toward grok-build, one probe per commit

Each B1 probe is one commit with a native test and a GREEN Live run. A probe
that fails stays in its table with the observed error text.

Landed by this commit: `feat(grok): omit store on Grok Responses egress` omits `store` from `GrokResponsesRequest`.

### Stage B2: extend with Grok-native abilities, one ability per commit

Each B2 row is one semantic commit that adds the stock-compatible entry
(config or Provider seam), the whitelist emit row, any ingress recognition,
native tests at both seams, and a Live Story when the ability is
user-visible. The P0 x_search probe runs before any B2 work on x_search so
the extension builds on a proven path.

B1 and B2 are independent of each other and of Stage A's ordering; Stage A
lands first because it is the surface both build on.

### Delivery

PR to `grok/rust-v*` runs Cargo. Push runs six targets and Live. Publish
follows [`release.md`](./release.md): only from a GREEN run whose SHA is the
branch head or is separated from it by inert paths alone. A whitelist commit
that lands after a GREEN run is a proof input and needs its own GREEN run
before `grok/release.py publish`; a docs-only commit does not.

## Tests

- `codex-rs/codex-api/src/grok_request_tests.rs`: dialect identity
  (`for_provider`), copy-only canonical request, one accepted fixture per
  `ResponseItem` variant and tool type that must build without an
  OpenAI-only key, rejections,
  and the stock OpenAI identity control (`stock_openai_projection_remains_identity`).
- `codex-rs/codex-api/src/grok_stream_tests.rs` and
  `codex-rs/core/tests/suite/grok_hosted_stream.rs`: Grok interleaved
  output streams are sequenced by `output_index` at ingress; stock OpenAI
  remains identity in `sse/responses.rs`.
- `codex-rs/core/tests/suite/grok_hosted_x_search.rs`: a completed hosted
  `custom_tool_call` is replayed on the next Turn without a client
  `custom_tool_call_output`.
- `codex-rs/core/tests/suite/grok_web_search.rs` and
  `codex-rs/core/tests/suite/grok_reasoning_replay.rs`: keep asserting the
  outbound `/responses` body through the full core path.
- `codex-rs/model-provider/src/grok_provider_tests.rs`: ingress recognition
  (`is_provider_hosted_tool_call`) for every hosted name the capability
  surface lists, plus incomplete status and an unknown name. Widen it
  together with the B2 predicate change.
- Live: `grok/live` `go test -run '^TestGrok'` on the musl binary. The
  encrypted-reasoning continuation, image-edit, custom `apply_patch`, and
  hosted `web_search` (`TestGrokHostedWebSearch`) and hosted `x_search`
  (`TestGrokHostedXSearch`) Stories exercise the reasoning, hosted-replay,
  custom-tool, `WebSearchCall`, and Grok-native x_search rows above.
- Facts live in `grok/facts`, run by `grok-facts.yml` on dispatch or locally,
  and are not proof inputs.

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
shape in the test name. First ask whether the capability layer should have
hidden the producing feature; if so, fix it there and keep the whitelist
rejection as the guard. Otherwise the fix is a change to a Grok type or a
mapping row, never a new key removal on serialized JSON.

**Grok gains a native ability.** Add a row to the capability surface with
its grok-build shape, choose the stock-compatible entry seam, and schedule it
as a B2 commit. Do not hard-code it at egress.

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
- Hiding a feature at egress that the capability layer can hide. Egress
  rejection is the guard, not the policy.
- Surfacing a Grok ability without a stock-compatible entry seam.
