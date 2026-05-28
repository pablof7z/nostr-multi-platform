# `nmp` CLI

The `nmp` command is what makes NMP **adoptable instead of hand-wired**: it
scaffolds a new app and runs the codegen pipeline that produces the per-app
FFI crate (ADR-0010). It also installs app-owned source components from the
offline NMP component registry.

It ships in the `nmp-cli` crate (`crates/nmp-cli`). Install or run it:

```sh
cargo install --path crates/nmp-cli      # installs the `nmp` binary
# or, without installing:
cargo run -p nmp-cli -- <args>
```

> **Relationship to the legacy `nmp` binary.** The `nmp-codegen` crate also
> ships a `[[bin]] name = "nmp"` that only does `gen modules`. `nmp-cli` is
> the canonical superset (`init` **and** `gen modules`, the latter delegating
> to the `nmp-codegen` *library*, unmodified). Because two workspace members
> declare a `nmp` binary, prefer `cargo run -p nmp-cli --` /
> `cargo install --path crates/nmp-cli` over a bare workspace `cargo build`
> when you want the full CLI. `nmp-cli` does not modify `nmp-codegen`.

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
  Cargo.toml                 # workspace: members = ["crates/<name>-core"]
  nmp.toml                   # app manifest (NMP baseline + modules)
  README.md                  # per-app next steps
  crates/<name>-core/
    Cargo.toml               # nmp-core (absolute path) + serde
    src/lib.rs               # one Domain + View + Action module + descriptors
    examples/shell.rs        # minimal headless shell stub
```

The `<name>-core` crate is a **generic** example (an `EntryRecord` with a
domain store, a reactive view, and a validating action) — deliberately not
social-app-shaped. It demonstrates the kernel boundary: per cardinal
doctrine **D0**, app nouns live in `<name>-core`, never in `nmp-core`.

The skeleton compiles the moment it is scaffolded:

```sh
cd my-app
cargo check --all-targets            # green
cargo test -p my-app-core            # 2 tests pass
cargo run --example shell -p my-app-core
```

By default, `nmp init` writes `dependency_mode = "path"` and resolves the
NMP checkout path to the absolute location of the checkout that ran it, so the
skeleton builds from any directory (including a tempdir — see the integration
test `crates/nmp-cli/tests/init.rs`). Use `--nmp-version` for apps consuming a
published NMP release; use `--nmp-path` when developing NMP and an app together.

### `nmp gen modules [--manifest nmp.toml] [--out DIR] [--check]`

Invokes the `nmp-codegen` pipeline to emit the per-app FFI crate
`nmp-app-<name>` (`AppAction` / `AppUpdate` / `ViewSpec` enums, domain and
capability registrations, the `FfiApp` wrapper). Flags and defaults match
the legacy `nmp-codegen` binary exactly.

- `--manifest` — manifest path (default `nmp.toml`).
- `--out` — output directory (default `apps/<name>/nmp-app-<name>`).
- `--check` — regenerate to a scratch dir and diff; non-zero exit on drift.
  This is the deterministic-codegen CI gate.

```sh
nmp gen modules            # emit apps/<name>/nmp-app-<name>
nmp gen modules --check     # verify it is up to date (deterministic)
```

The generated `nmp-app-<name>/Cargo.toml` follows `[nmp]` in `nmp.toml`:

- `dependency_mode = "version"` emits versioned dependencies for `nmp-core`,
  `nmp-ffi`, and any `nmp-*` protocol modules. App-local modules remain path
  dependencies.
- `dependency_mode = "path"` emits local path dependencies against the NMP
  checkout and workspace layout. This is the mode for framework development.

### `nmp upgrade --to VERSION [--manifest nmp.toml]`

Moves an app manifest to a pinned NMP release baseline.

```sh
nmp upgrade --to 0.2.0
nmp gen modules
nmp gen modules --check
nmp doctor
```

The command updates the `[nmp]` section to `dependency_mode = "version"`,
records the target release, and rewrites direct `nmp-*` dependencies in local
app-module crates listed under `[modules].app`. Regeneration then rewrites
generated FFI crate dependencies to the matching `nmp-*` release train.
Component source updates remain explicit through `nmp update component` so
local app edits are not silently overwritten.

### `nmp doctor [--manifest nmp.toml]`

Reports the app name, dependency mode, pinned NMP version or checkout path, and
module count. It is the lightweight post-upgrade sanity check for app repos and
the seed for deeper toolchain checks.

### `nmp add component <id> [--path DIR] [--registry DIR] [--with ROLES]`

Copies an app-owned source component from the NMP component registry into an
app tree and records the installed upstream baseline in `nmp.components.lock`.

```sh
nmp add component swiftui/content-minimal
nmp add component swiftui/content-minimal --path /tmp/my-app --with example
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

Component contract:

- `crates/nmp-cli/registry/registry.toml` is the install authority. Showcase
  pages and docs may add copy, but ids, versions, targets, dependencies, and
  file mappings must mirror the CLI manifest.
- Content components are copied source owned by the app after install. They
  are not linked framework packages.
- Rust owns content structure through `nmp-content` / `ContentTreeWire`.
  Native code decodes and renders that tree.
- Components are pure renderers. They do not fetch, retry, cache, route, or
  decide policy. Apps hydrate display models such as `NostrQuoteCardModel`
  from their own state.
- User actions leave components through `NostrContentCallbacks` /
  `LocalNostrContentRenderer`; the embedding app decides navigation and OS
  capability execution.

## Verification

`crates/nmp-cli/tests/init.rs` is the end-to-end gate:

1. `nmp init` into a fresh tempdir.
2. `cargo check --all-targets` on the scaffold → green.
3. `cargo test -p <name>-core` → skeleton tests pass.
4. `nmp gen modules` → succeeds, emits the FFI crate.
5. `nmp gen modules --check` → no drift (codegen is deterministic).

A second test asserts invalid app names are rejected.

`crates/nmp-cli/tests/upgrade.rs` covers the release-consumer path:

1. `nmp upgrade --to <version>` rewrites `[nmp]`.
2. `nmp gen modules` emits versioned `nmp-core` / `nmp-ffi` dependencies.
3. `nmp doctor` reports the pinned baseline.

`crates/nmp-cli/tests/component.rs` covers component installation:

1. `nmp add component swiftui/content-minimal --with example`.
2. Dependency installation for `swiftui/content-core`.
3. Lock-file creation with installed source hashes.
4. Duplicate and unknown-component rejection.
