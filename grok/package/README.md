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

## Native target builds and package smoke

The branch-local `grok.yml` runs separate Linux musl and macOS ARM build jobs on
its ordinary PR and version-line push events. The `build-grok` action builds each
target's runtime and Code Mode host once with the locked stock release profile,
uses the stock helper layout/resolvers, and stages the resulting complete
package. Linux embeds the final stripped `bwrap` digest before building Codex.
Stock pinned V8 artifacts and their checksum validation remain in use.

Each target executes the staged launcher and runtime, checks the reported version
against package metadata, starts the Code Mode host's help path, exercises its
packaged `rg`, `zsh` and Linux `bwrap`, and runs the existing nonempty
`TestNativeBasicFixture` and `TestNativeShippedCatalog` selection through the
packaged launcher. The Basic fixture uses a controlled local backend for both
retained models; the catalog check uses the package's shipped profile/catalog.
These checks do not make a real-provider request or establish backend claims.

The complete package is uploaded as
`grok-SOURCE_SHA-TARGET-RUN_ID-RUN_ATTEMPT`, containing
`grok-SOURCE_SHA-TARGET.tar.gz`. Executable modes survive the archive. A separate
`grok-package-evidence-SOURCE_SHA-TARGET-RUN_ID-RUN_ATTEMPT` artifact records the
package file hashes, source/target, runtime and launcher versions, archive hash,
and actual fixture log. Both artifacts have 30-day retention. The package upload
runs only after its target's complete smoke succeeds. Partial evidence can be
retained on failure and is not a successful package claim.

Select the explicit run/attempt and target on the Actions page, then extract the
tar archive from that target's artifact. Follow its included `INSTALL.md` with a
new dedicated Home. PR builds identify the tested merge SHA and are diagnostic
packages; they are not automatically accepted releases. A missing, failed or
expired target artifact remains unavailable. Each target is a distinct job, so
one target cannot hide the other's failure. Native `Cargo` continues to depend
only on `grok-proof.yml`, independently of package or backend availability.

For exact same-run and diagnostic artifact selection and owned Live invocation,
see `ARTIFACTS.md`. These capabilities remain subject to actual Actions proof.
Manual workflow availability and real-backend execution must be verified
separately; their declarations are not execution evidence.
No stock `main` change or registered manual entrypoint is implied. Local checks
are development diagnostics; package/build acceptance requires the exact Actions
subject and actual target results.
