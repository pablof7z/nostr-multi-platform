# M17 — v1 release

> Part of the [Build & Validation Plan](../plan.md). Arc 3 — WoT + cross-platform + release (M12 Wallet deferred post-v1).

**Release doctrine.**

NMP ships as a coordinated release train. Apps pin one NMP baseline and
upgrade through generated migrations, not by mixing arbitrary crate versions.
Path dependencies are the framework-development mode; versioned `nmp-*`
dependencies are the app-consumer mode.

The release manifest lives at `release/nmp-release.toml`. It is the
machine-readable authority for:

- the release train version;
- public crates vs. private examples/apps/tools;
- schema versions for generated/runtime contracts;
- the component registry revision used by `nmp.components.lock`;
- the app upgrade command.

**Scope.**

- Resolve naming (`aim.md` §7.7).
- Publish every `[[public_crates]]` entry from `release/nmp-release.toml` to
  crates.io as one coordinated version.
- Publish CLI to npm as `@<name>/cli`, wrapping the same Rust binary.
- Keep private app/example/tool crates unpublishable and out of the release
  train.
- Support `nmp init --nmp-version <version>` for release consumers and
  `nmp init --nmp-path <checkout>` for framework development.
- Support `nmp upgrade --to <version>` as the app-facing version bump and
  regeneration entrypoint.
- Tag release; publish bindings; deploy example apps; write release announcement.

**Automation.**

- `ci/check-release-manifest.sh` verifies every public release crate is
  classified in the release manifest and has required crates.io metadata.
- `.github/workflows/release-readiness.yml` is the manual/tag release gate. It
  checks the manifest and runs `cargo package --list` for each public crate.
- Codegen reads `[nmp]` from `nmp.toml`: `dependency_mode = "version"` emits
  versioned `nmp-core`, `nmp-ffi`, and `nmp-*` module deps; `path` emits local
  checkout deps.

**Exit gate.**

- Public availability on crates.io and npm.
- `nmp init --nmp-version <v>` creates a scaffold whose generated FFI crate
  depends on published `nmp-*` versions.
- `nmp upgrade --to <v>` updates `nmp.toml`, regeneration is deterministic, and
  `nmp doctor` reports the app's pinned baseline.
- Release-readiness workflow is green for the release tag.
- Three external developers ship a real app within 30 days of release.
- v1 release report in `docs/perf/v1/release.md`.
