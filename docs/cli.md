# `nmp` CLI

The `nmp` command is what makes NMP **adoptable instead of hand-wired**: it
scaffolds a new app as a thin **composition shell** over reusable framework
installer crates, and installs app-owned source components from the offline NMP
component registry.

Per **ADR-0069**, a downstream app has an explicit Rust composition root:
substrate, selected protocol features, app features, capability contracts, and
product defaults are visible in app code rather than hidden behind a production
preset.

It ships in the `nmp-cli` crate (`crates/nmp-cli`). Install or run it:

```sh
cargo install --path crates/nmp-cli      # installs the `nmp` binary
# or, without installing:
cargo run -p nmp-cli -- <args>
```

> **Relationship to the `nmp-codegen` binary.** The `nmp-codegen` crate ships the
> `nmp-codegen` binary for focused host-helper emitters (`gen typed-decoders`,
> projection-cache, keyed-ref-cache, feed/action helpers, registry builtin
> files). `nmp-cli` is the
> developer-facing CLI (`init`, `add`/`update component`, `upgrade`,
> `export`). Use `cargo run -p nmp-cli --` / `cargo install --path crates/nmp-cli`
> for the full developer CLI.

## Commands

### `nmp init <app-name> [--path DIR] [--nmp-version VERSION | --nmp-path DIR]`

Scaffolds a new, immediately-buildable NMP app.

```sh
nmp init my-app                 # scaffolds ./my-app
nmp init my-app --path /tmp/x   # scaffolds /tmp/x
nmp init my-app --nmp-version 0.2.0
nmp init my-app --nmp-path ../nostr-multi-platform
```

App-name rules: lowercase letters, digits, and single hyphens; must start
with a letter and end with a letter or digit (`my-app`, `notes2`). `Demo`,
`1app`, `my--app`, `my_app`, `app-` are rejected.

Produced layout:

```text
<root>/
  Cargo.toml                 # workspace: core + app-owned UniFFI facade
  nmp.toml                   # NMP dependency policy (read by upgrade)
  action-builders.json       # app-local typed action-builder contract
  generated/                 # Swift/Kotlin/TS builders from action-builders.json
  ci/check-uniffi-bindings.sh # Swift/Kotlin UniFFI binding-generation check
  README.md                  # per-app next steps
  crates/<name>-core/
    Cargo.toml               # nmp-substrate + selected protocol crates + nmp-native-runtime + nmp-core + serde
    src/lib.rs               # explicit register() root + example domain
    src/entry_action.rs      # app-owned typed ActionPayload + ActionModule
    src/entry_view.rs        # app-owned reactive read model
    schema/add_entry.fbs     # app-owned FlatBuffers payload schema
    examples/shell.rs        # NmpAppBuilder → register → start
  crates/<name>-app/
    Cargo.toml               # app-owned UniFFI cdylib/staticlib/rlib
    src/lib.rs               # setup_scaffolding! + facade-local types
```

The `<name>-core` crate is a **thin composition shell** (ADR-0069): its
`register` function installs the reusable NMP substrate floor explicitly, then
leaves selected protocol features and app-owned modules visible in the app root.
It also carries a **generic** example domain (an `EntryRecord` with a reactive
view and a validating action), deliberately not social-app-shaped, to
demonstrate the kernel boundary: per cardinal doctrine **D0**, app nouns live in
`<name>-core`, never in `nmp-core`.

The `<name>-app` crate is the app-owned native doorway. It calls
`uniffi::setup_scaffolding!()`, owns `NmpApp` by value inside an `Arc<Facade>`
UniFFI object, defines facade-local callback/record types, and delegates
lifecycle, update, capability, and action-dispatch mechanics to
`nmp-uniffi-support`. The scaffold therefore teaches one native path: app shells
link the facade and dispatch generated action bytes; they do not hand-write raw
`extern "C"` symbols.

The starter also includes one app-local typed action end to end:
`action-builders.json` declares `<name>.entries.add`, `schema/add_entry.fbs`
owns the wire payload, generated Swift/Kotlin/TS builders emit
`GeneratedActionBuilders.addEntry(...)`, and `entry_action.rs` decodes those
bytes into `AddEntryAction` through the app's `EntryActionModule`. The action
publishes the starter app-private event kind, and `entry_view.rs` declares the
event dependency, maintains bounded Rust-owned view state, and snapshots rows
for rendering.

The shell compiles the moment it is scaffolded:

```sh
cd my-app
cargo check --all-targets                  # core + facade green
cargo test -p my-app-core                  # core tests pass
cargo test -p my-app-app                   # facade tests pass
cargo run --manifest-path /path/to/nostr-multi-platform/Cargo.toml -p nmp-codegen -- gen action-builders --registry action-builders.json --check
bash ci/check-uniffi-bindings.sh            # Swift/Kotlin facade bindings generate
cargo run --example shell -p my-app-core   # app register → start → stop
```

By default, `nmp init` writes `dependency_mode = "path"` and resolves the NMP
checkout path to the absolute location of the checkout that ran it, so the shell
builds from any directory (including a tempdir — see the integration test
`crates/nmp-cli/tests/init.rs`). Use `--nmp-version X.Y.Z` for apps consuming a
published NMP release (git-rev pins on `github.com/pablof7z/nostr-multi-platform`
at tag `vX.Y.Z` — consumers pin NMP by git rev, see
`docs/architecture/external-consumers.md`); use `--nmp-path` when developing NMP
and an app together.

### `nmp upgrade --to VERSION [--manifest nmp.toml]`

Moves an app manifest to a pinned NMP release baseline and repoints the app
crate's `nmp-*` dependencies at the new git tag.

```sh
nmp upgrade --to 0.4.0
```

The command updates the `[nmp]` section to `dependency_mode = "version"`,
records the target release, and rewrites direct `nmp-*` dependencies in local
app-module crates listed under `[modules].app` to git-rev pins at the new tag
(the same shape `nmp init --nmp-version` emits). Component source updates remain
explicit through `nmp update component` so local app edits are not silently
overwritten.

### `nmp add component <id> [--path DIR] [--registry DIR] [--with ROLES]`

Copies an app-owned source component from the NMP component registry into an
app tree and records the installed upstream baseline in `nmp.components.lock`.

```sh
nmp add component swiftui/content-minimal
nmp add component swiftui/content-minimal --path /tmp/my-app --with example
nmp add component swiftui/component-host --with fixture
```

- `--path` — app root to install into (default: current directory).
- `--registry` — filesystem registry path for tests or local registry authoring
  (default: the built-in offline registry embedded in `nmp-cli`).
- `--with` — comma-separated optional file roles to include. Source files are
  always installed; roles such as `example`, `doc`, `test`, and `fixture` are
  opt-in.

The built-in registry ships installable SwiftUI and Compose content kits. The
minimal SwiftUI bundle depends on `swiftui/content-core` and writes:

```text
Components/NostrContent/NostrContentRenderer.swift
Components/NostrContent/NostrMinimalContentView.swift
nmp.components.lock
```

Re-running `add component` for an already-installed component fails instead of
overwriting app-owned files. The lock records component versions, target files,
source paths, roles, and source hashes so `nmp update component` can later
compute a safe source update against local app edits.

The full content renderers are:

```sh
nmp add component swiftui/content-view
nmp add component compose/content-view
```

Each full renderer installs the platform `content-core` wire mirror, media
grid, quote card, grouping logic, and main `NostrContentView` dispatcher.
Reference-first user and content components should be paired with one
app-root component host. `swiftui/component-host --with fixture` and
`compose/component-host --with fixture` copy no-kernel conformance providers for
`refs.profile`, `refs.event`, and `refs.event.envelopes`; production apps
replace those fixtures with their real shell bridge.

Component contract:

- `crates/nmp-component-registry/registry/registry.toml` is the install authority. Showcase
  pages and docs may add copy, but ids, versions, targets, dependencies, and
  file mappings must mirror the component registry manifest.
- Content components are copied source owned by the app after install. They
  are not linked framework packages.
- Rust owns content structure through `nmp-content` / `ContentTreeWire`.
  Native code decodes and renders that tree.
- Components are pure renderers. They do not fetch, retry, cache, route, or
  decide policy. Apps hydrate display models such as `NostrQuoteCardModel`
  from their own state.
- Component packages must not import runtime, legacy native compatibility shims,
  wasm Worker handles, or kernel handles directly. They consume the app-level
  component host/provider and projection rows; app-specific rich projections
  belong in the app Rust core.
- User actions leave components through `NostrContentCallbacks` /
  `LocalNostrContentRenderer`; the embedding app decides navigation and OS
  capability execution.

## Verification

`crates/nmp-cli/tests/init.rs` is the end-to-end gate:

1. `nmp init` into a fresh tempdir.
2. Assert the scaffold is a composition shell: `register` installs the NMP
   substrate explicitly, the example drives `NmpAppBuilder`, and the native
   doorway is exactly one app-owned UniFFI facade over `nmp-uniffi-support`.
3. `cargo check --all-targets` on the scaffold → green (links the live
   `nmp-substrate` / selected protocol / `nmp-native-runtime` / `nmp-core`
   crates).
4. `cargo test -p <name>-core` and `cargo test -p <name>-app` → skeleton tests
   pass.
5. The generated `action-builders.json` registry passes the normal
   `nmp-codegen gen action-builders --registry ... --check` drift gate.
6. `bash ci/check-uniffi-bindings.sh` builds the app-owned facade cdylib and
   runs `uniffi-bindgen --library` for Swift/Kotlin.

A second test asserts invalid app names are rejected.

`crates/nmp-cli/tests/upgrade.rs` covers the release-consumer path:

1. `nmp upgrade --to <version>` rewrites `[nmp]`.
2. The app crate's `nmp-*` dependencies become git-rev pins at the new tag.
3. The app crate's dependency rewrite is verified directly from `Cargo.toml`.

`crates/nmp-cli/tests/component.rs` covers component installation:

1. `nmp add component swiftui/content-minimal --with example`.
2. Dependency installation for `swiftui/content-core`.
3. Lock-file creation with installed source hashes.
4. Duplicate and unknown-component rejection.
