# Install Grok

This distribution is a downstream Codex product with the Grok Provider adaptations.
It is not the default Codex installation and it is not the xAI grok-build
installation.

The Actions distribution artifact ships the complete configuration contract as readable files:

```text
config.toml.example
models.json
INSTALL.md
bin/
```

There is no installer. A human or an assisting agent chooses the destination
and performs the file operations deliberately.

## Important: use a dedicated product Home

This product must use its own `CODEX_HOME`.

Do not use the normal Home of another product:

```text
~/.codex   # default Codex
~/.grok    # Grok / grok-build
```

Do not point this product at an existing Codex or Grok Home merely because the
directory already exists. Codex Home contains configuration, sessions, memory,
and other product state. Sharing it mixes product state and makes later updates
ambiguous.

The packaged `grok` command requires `CODEX_HOME` to be set explicitly and
refuses the two common shared-home paths above.

## Fresh setup

Choose a dedicated directory. The distribution intentionally does not prescribe one
fixed name.

For example, in a shell:

```sh
export CODEX_HOME=/absolute/path/to/your/dedicated-product-home
mkdir -p "$CODEX_HOME"
```

Before copying files, inspect the directory. For a fresh Home it should not
contain another product's `config.toml`, sessions, memories, or model catalog.

Copy the release configuration:

```sh
cp config.toml.example "$CODEX_HOME/config.toml"
cp models.json "$CODEX_HOME/models.json"
```

The shipped profile contains:

```toml
model = "grok-4.7"
model_provider = "grok"
model_catalog_json = "models.json"
```

The catalog path is relative to `config.toml`, so `config.toml` and
`models.json` belong beside each other in the chosen Home.

Set credentials:

```sh
export GROK_API_KEY=...
```

GitHub Actions artifacts do not preserve Unix executable bits. After download,
mark the Unix programs executable:

```sh
chmod +x bin/grok bin/grok-bin bin/codex-code-mode-host
[ ! -f bin/bwrap ] || chmod +x bin/bwrap
```

Then run directly from the downloaded distribution artifact:

```sh
./bin/grok
```

or put the artifact's `bin` directory on `PATH` and run `grok`.

Current distribution artifacts target:

- `x86_64-unknown-linux-musl` for servers, CI, and Linux use;
- `aarch64-apple-darwin` for macOS ARM.

Windows is not currently a delivery target. The PowerShell wrapper keeps the
same explicit-`CODEX_HOME` contract for any future Windows artifact.

## Updating an existing product Home

A new distribution may contain changed `config.toml.example` or `models.json`.
The distribution does not automatically overwrite either file in your Home.

Inspect the new distribution assets first.

- Treat your existing `config.toml` as user-owned. Compare it with the new
  example and merge desired changes deliberately.
- If your Home uses the shipped product catalog unchanged, compare the new
  `models.json` and replace it deliberately when you want the new catalog.
- If `model_catalog_json` points at a custom catalog, do not replace that
  catalog with the shipped file.
- Keep `model_catalog_json = "models.json"` only when the chosen Home really
  contains the shipped catalog under that relative name.

There is intentionally no automatic migration from `~/.codex`, `~/.grok`,
or an older downstream Home.

## Instructions for an assisting agent

An agent may help with installation, but it must treat file writes as user-state
changes.

Before writing:

1. Ask for or identify the intended dedicated product `CODEX_HOME`.
2. Inspect the destination.
3. Warn the human if the destination is `~/.codex`, `~/.grok`, or contains
   state that appears to belong to another Codex/Grok installation.
4. Do not overwrite an existing `config.toml` or custom model catalog without
   explicit human approval.
5. Prefer creating a new dedicated Home when ownership is unclear.

For a fresh approved Home, copy `config.toml.example` to `config.toml` and
copy the complete distribution `models.json` beside it. Then set/use `CODEX_HOME`
for every invocation of this product.

For an update, show the human the material differences in the distribution assets
before replacing or merging user state.

## Model catalog contract

The shipped catalog contains the product request slugs:

```text
grok-4.7
grok-4.6
```

`grok-build` is not a shipped request slug. The production route accepts it
for some operations but rejects Responses `reasoning.effort`. The xAI
grok-build CLI has its own model-variant routing before serialization; this
product does not reproduce that routing.

Remote `/models` is evidence/revalidation input. It is not runtime catalog
authority. The Rust static catalog is only a compatibility fallback when
`model_catalog_json` is absent.
