# rust-v0.158.0 evidence classification

Evidence for the reconstructed candidate on `carry/grok-rust-v0.158.0`.
This file does not grant source authority. Native owner proof is the
implementation closure. Facts are backend observations. Live success does
not make a source SHA accepted.

Baseline evidence lives on `grok/rust-v0.157.1`. Classifications below are
against exact stock `064c6b8c737f5b41d171fdda80bd9ef10ad06eb3` plus the
closed owners on this branch.

Real-provider Fact probes and Live scenarios were not rerun for this
candidate. Rerun is not required for the retained claims: the product
behavior is covered by the named native tests, and Live still consumes a
packaged binary owned by distribution rather than by this reconstruction.
`GROK_FACTS` probes stay skipped unless `GROK_FACTS=1` and `GROK_API_KEY`
are set.

## Native closure already proven

HEAD `ef5fc14e80f828e3df6003279339802ae6cd5c49` run
`36660075929`: #320, #321, #322, #323, #327, #328, #329, stock
compile/lint, all-owner composition, and native carry required all
succeeded.

## Stories

| Story | Class | 0.158 proof |
| --- | --- | --- |
| Provider profile startup | KEEP | shipped `grok/dist/config.toml.example` still selects `grok` / `grok-4.7`; Facts `TestLoadShippedProfile` |
| Encrypted reasoning continuation | KEEP | `grok_reasoning_replay` native test |
| Child agent Provider inheritance | KEEP | #329 App Server `grok` tests on the composition run |
| Image generation and history edit | UPDATE | Grok image dialect retained; stock transparent-background and file-backed edits stay stock-owned; unverified `ImageReference::File` is rejected on the Grok dialect. Native `grok_images` and image-extension tests passed. Live `TestGrokImageGenerationEdit` not rerun |
| Structured exact-match edit | KEEP | `structured_edit` and `structured_edit_hooks` native suites |
| Hosted web_search turn | KEEP | `grok_hosted_stream` / `grok_web_search` native tests |
| web_search allowlist | KEEP | retained fail-closed filter projection |
| web_search excluded domains | KEEP | `grok_emits_web_search_excluded_domains_from_stock_config`; generated schema now includes `excluded_domains` |
| Hosted x_search turn and replay | KEEP | `grok_hosted_x_search_follow_up_replays_unpaired_custom_tool_call` |
| x_search date window | KEEP | `grok_emits_provider_configured_x_search_window` |
| Bundled catalog visibility | KEEP | #329 `cargo test -p codex-app-server --test all grok` |
| Provider binding across fork, resume, compaction | KEEP | same #329 suite |

## Fact probes

Unit tests in `grok/facts` pass without network. Each `TestFact*` probe
below is KEEP as a historical observation and was not rerun.

| Probe | Class | Note |
| --- | --- | --- |
| `TestFactWebSearchExternalWebAccessRejected` | KEEP | not rerun |
| `TestFactReasoningNullContentWithBlobRejected` | KEEP | not rerun |
| `TestFactReasoningTypedContentWithBlob` | KEEP | not rerun |
| `TestFactFunctionStrict` | KEEP | not rerun; strict remains omitted on the Grok projection |
| `TestFactInputStatusOnHostedItems` | KEEP | not rerun; Grok projection still omits hosted `status` |
| `TestFactCustomToolCallReplayRequiresID` | KEEP | native hosted-call replay covers the client invariant |
| `TestFactWebSearchCallReplayRequiresAction` | KEEP | not rerun |
| `TestFactParallelToolCallsStoreClientMetadata` | KEEP | not rerun |
| `TestFactIncludeEncryptedReasoning` | KEEP | not rerun |
| `TestFactWebSearchAllowedDomains` | KEEP | native allowlist behavior retained |
| `TestFactWebSearchExcludedDomains` | KEEP | native excluded-domain test passed |
| `TestFactXSearchDateWindow` | KEEP | native provider date-window test passed |
| `TestFactTextVerbosityRejectedOrIgnored` | KEEP | not rerun |
| `TestFactModelRouteGrokBuild` | KEEP | observation only; `grok-build` is not product authority |
| `TestFactModelRouteGrok47` | KEEP | not rerun; shipped model remains `grok-4.7` |
| `TestFactModelRouteGrok46` | KEEP | not rerun |

## Live scenarios

Each Live entry is KEEP and was not rerun. There is no 0.158 distribution
artifact, and Live does not define reconstruction.

| Scenario | Class |
| --- | --- |
| `TestGrokBasic` | KEEP, not rerun |
| `TestGrokPinnedPreviousModel` | KEEP, not rerun |
| `TestGrokEncryptedReasoningContinuation` | KEEP, not rerun |
| `TestGrokCollaboration` | KEEP, not rerun |
| `TestGrokImageGenerationEdit` | UPDATE, not rerun; see image story |
| `TestGrokStructuredEdit` and approval/replace-all variants | KEEP, not rerun |
| `TestGrokHostedWebSearch` | KEEP, not rerun |
| `TestGrokHostedWebSearchAllowlist` | KEEP, not rerun |
| `TestGrokHostedWebSearchExcludedDomains` | KEEP, not rerun |
| `TestGrokHostedXSearch` | KEEP, not rerun |
| `TestGrokHostedXSearchDateWindow` | KEEP, not rerun |

Harness-only Live tests (`TestLastAgentMessageSkipsEmptyItems` and the
other non-`TestGrok*` helpers) are KEEP as harness mechanics. They are not
product claims.

No scenario is DROP or UNKNOWN. The image Live story is UPDATE because the
stock image seam moved; the retained Grok dialect is already proven
natively.
