# One Grok-bound App Server process serves exactly the release-bundled Grok catalog

## User story

As a Grok user, the models I can see and pick in a Grok-bound App Server are
exactly the models the exact Grok release verified for Grok. I do not see
another Provider's models mixed in, and I do not see a Grok model the release
did not bundle.

## Real path

```text
exact release-bundled Grok catalog
  -> the Provider Profile the App Server process was started with
  -> App Server model/list
  -> model picker
```

One App Server process serves one Provider Profile. Using another Provider
means starting another process. Remote Provider catalog observations are
prerequisite release evidence. They are not read or merged into the Grok
runtime catalog.

## Acceptance

**Given** a Home whose `model_provider` is the shipped Grok profile,
**when** the agent reads App Server `model/list`,
**then**:

- the response contains exactly the release-bundled Grok catalog (currently the
  single `grok-4.6` entry) through the stock `Model` DTO;
- the release-bundled identity, reasoning efforts, default effort, and
  Multi-Agent version reach `model/list` unchanged;
- no model from another Provider Profile appears;
- runtime availability of a remote `/models` route does not change the
  catalog; and
- a Home with only the stock OpenAI/ChatGPT profile does not list `grok-4.6`.

## Partial success is not completion

- `model/list` lists a Grok slug the exact release catalog does not contain.
- `model/list` under the Grok profile contains a model owned by another
  Provider.
- A remote catalog probe passes but the exact artifact does not expose the
  verified release model.

## Material failure boundaries

- Catalogs from different Providers must not merge in one process.
- Remote `/models` must not become a runtime catalog authority.

## What this does not prove

This Story does not prove remote catalog freshness or that any listed model can
complete inference ([grok-provider-profile-startup](./grok-provider-profile-startup.md)
proves one Turn). It does not prove Thread Provider binding across lifecycle
events, hosted tools, Mini routing, or a picker that shows several Providers at
once.

## Proof plan

### Preconditions

- The exact source contains the verified Grok catalog and its stock model
  projection.
- Native catalog and stock compatibility tests passed for that source.

### Proof-run invocation budget

None. This Story is a property of the exact source, proven by a real App
Server on a mock gateway.

### Secret-safe evidence

Record source identity, the gate that proved it, Provider names, and model
counts. Negative evidence: no credentials, headers, raw bodies, or account
identifiers.

## Stock compatibility control

An isolated Home with only the stock OpenAI/ChatGPT Provider Profile keeps the
upstream `model/list` behavior. The stock `model/list` suite proves this at
the same fixed point.

## Executable contract

`list_models_uses_grok_release_catalog_through_stock_model_dto` in
`codex-rs/app-server/tests/suite/v2/grok_model_list.rs` starts a real App
Server on a Grok-profile Home and asserts that `model/list` returns exactly
the release-bundled `grok-4.6` `Model` DTO. The stock `model/list` suite in
the same crate is the stock compatibility control. Both run through native
`cargo test` in `grok-checks`.

## Last proven

`grok-checks` on the `rust-v0.153.4` line
([Harness-X-Harness/codex#107](https://github.com/Harness-X-Harness/codex/pull/107)).
