# Package staging

This independently callable part of #339 stages complete, explicit prebuilt
inputs. It does not compile binaries, download dependencies, install a user Home
or select artifacts from another run. Use Python 3.11 or later from a clean
checkout containing this command:

```sh
python3 grok/package/stage.py \
  --source-sha "$(git rev-parse HEAD)" \
  --target x86_64-unknown-linux-musl \
  --runtime /build/codex \
  --code-mode-host /build/codex-code-mode-host \
  --rg /dependencies/rg \
  --zsh /dependencies/zsh \
  --bwrap /build/bwrap \
  --output /existing-parent/new-package
```

For `aarch64-apple-darwin`, provide that target's inputs and omit `--bwrap`.
Other targets are rejected. Every executable input must already exist as a
nonempty, executable regular file. Stock `codex_package.layout` owns directory
assembly and validation; Grok adds its launcher, unmodified product profile and
catalog, installation instructions, license and source/target/hash metadata.

The selected SHA must equal checkout HEAD; owned staging/assets must be tracked
and unchanged. The build producer must bind prebuilt bytes to that source and
target. The metadata records their exact hashes; it does not infer executable
architecture or source origin from arbitrary filenames. Linux production builds
must embed the digest of the same final `bwrap` bytes supplied here.

The destination must not exist, and its parent must already exist. There is no
force/replace mode. A staging failure returns nonzero; an output failure can
leave a partial directory for diagnosis. It cannot have a completed
`grok-package.json`. Use a new destination after diagnosing the original failure.

Run the deterministic mechanism tests with:

```sh
python3 grok/package/test_stage.py -v
```

They execute the staging command and launcher with synthetic executable files.
They prove layout, source checks, exact asset/hash retention, input rejection,
Home protection and argument forwarding. They do not execute a real Linux or
macOS product. The shared native Actions proof runs this same command; local
results remain development diagnostics, and formal evidence is Actions-only.

This slice does not complete #339. Real build actions, same-run build-to-Live
dependencies, explicit diagnostic artifact selection, available credentials,
actual manual workflow registration and full platform/package execution remain
separate unimplemented or unverified obligations. No stock `main` change or
registered manual entrypoint is implied by this script.
