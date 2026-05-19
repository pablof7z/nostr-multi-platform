# `nmp` CLI

The `nmp` command is what makes NMP **adoptable instead of hand-wired**: it
scaffolds a new app and runs the codegen pipeline that produces the per-app
FFI crate (ADR-0010).

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

### `nmp init <app-name> [--path DIR]`

Scaffolds a new, immediately-buildable NMP app.

```sh
nmp init my-app                 # scaffolds ./my-app
nmp init my-app --path /tmp/x   # scaffolds /tmp/x
```

App-name rules: lowercase letters, digits, and single hyphens; must start
with a letter and end with a letter or digit (`my-app`, `notes2`). `Demo`,
`1app`, `my--app`, `my_app`, `app-` are rejected.

Produced layout:

```text
<root>/
  Cargo.toml                 # workspace: members = ["crates/<name>-core"]
  nmp.toml                   # app manifest (kernel + protocol + app modules)
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

This works because `nmp init` resolves the `nmp-core` path dependency to the
**absolute** location of the checkout that ran it, so the skeleton builds
from any directory (including a tempdir — see the integration test
`crates/nmp-cli/tests/init.rs`).

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

**Monorepo-path caveat.** The generated `nmp-app-<name>/Cargo.toml`
references the kernel monorepo-relatively (`../../../crates/nmp-core`), per
ADR-0010 — that crate is designed to live inside an `nmp` checkout. To build
the generated FFI crate, place the app under an `nmp` checkout so
`crates/nmp-core` resolves, and add `apps/<name>/nmp-app-<name>` to the
workspace members. The hand-written `<name>-core` skeleton has no such
constraint (its `nmp-core` path is absolute).

## Verification

`crates/nmp-cli/tests/init.rs` is the end-to-end gate:

1. `nmp init` into a fresh tempdir.
2. `cargo check --all-targets` on the scaffold → green.
3. `cargo test -p <name>-core` → skeleton tests pass.
4. `nmp gen modules` → succeeds, emits the FFI crate.
5. `nmp gen modules --check` → no drift (codegen is deterministic).

A second test asserts invalid app names are rejected.
