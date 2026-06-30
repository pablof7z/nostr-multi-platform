use std::env;

mod cli;
mod cli_action_builders;
mod cli_action_contract;
mod cli_builtin;

fn main() {
    match run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("nmp: {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 2 || args[0] != "gen" {
        return Err(help());
    }
    let subcommand = args.remove(1);
    args.remove(0); // drop "gen"
    let h = help();
    match subcommand.as_str() {
        // V6 Stage 1 — Swift `Decodable` emitter. Reads projection schema
        // documents (default: stdin) and writes Swift to `--out`. See
        // `crates/nmp-codegen/src/swift.rs` for the emitter itself.
        "swift" => cli::run_gen_swift(args, &h),
        // V6 Stage 4 (consumer-side) — generated typed-FlatBuffer-sidecar
        // decoders. Writes `TypedProjectionDecoders.generated.swift` from the
        // registry's `typed_sidecar` metadata; no schema-document stdin needed.
        "typed-decoders" => cli::run_gen_typed_decoders(args, &h),
        // ADR-0055 R3-S3 — generated `ProjectionMergeCache`. Writes
        // `ProjectionCache.generated.swift` or `ProjectionCache.kt` (per
        // `--platform`) from the same registry as `typed-decoders`;
        // implements the D3-3 merge algorithm.
        "projection-cache" => cli::run_gen_projection_cache(args, &h),
        // ADR-0063 Lane A (#1671) — generated per-key (row-keyed) reference
        // cache for keyed projections (`refs.profile` / `refs.event`). Writes
        // `KeyedRefCache.generated.swift` or `KeyedRefCache.kt` (per
        // `--platform`) from `KEYED_PROJECTIONS`; decodes `RefRowDeltaBatch`.
        "keyed-ref-cache" => cli::run_gen_keyed_ref_cache(args, &h),
        // ADR-0064 §3 (#1783) / #2411 — generated typed action-builders.
        // Writes native/web builders from `ACTION_BUILDERS` or from an app-local
        // static `--registry` contract; emits the host-facing typed write
        // builders that construct `DispatchEnvelope` bytes for the byte doorway.
        "action-builders" => cli_action_builders::run_gen_action_builders(args, &h),
        // #1939 — generated compact Markdown view of ACTION_CONTRACT for PR
        // review. Prints to stdout unless `--out <path>` is provided.
        "action-contract-report" => cli_action_contract::run_gen_action_contract_report(args, &h),
        // ADR-0053 / Workstream-E4 — generated `KERNEL_BUILTIN_PROJECTION_KEYS`
        // Rust const for `nmp-core`. Writes
        // `crates/nmp-core/src/kernel/update/builtin_projection_keys.generated.rs`
        // from the SAME projection registry as `typed-decoders`; no stdin.
        "builtin-keys" => cli_builtin::run_gen_builtin_keys(args, &h),
        // #1723 — generated `BUILTIN_PROJECTION_DEPENDENCIES` revision table for
        // `nmp-core`. Writes
        // `crates/nmp-core/src/kernel/projection_rev/builtin_projection_deps.generated.rs`
        // from the SAME projection contract as `builtin-keys`; no stdin.
        "builtin-deps" => cli_builtin::run_gen_builtin_deps(args, &h),
        // #1723 — generated `DRAIN_PROJECTION_KEYS` + `CONDITIONAL_PRESENCE_KEYS`
        // presence-classification sets for `nmp-core`'s projection_rev module,
        // from the contract's `presence_policy` column. No stdin.
        "presence-keys" => cli_builtin::run_gen_presence_keys(args, &h),
        // #1723 — generated per-projection producer constants (`*_SCHEMA_ID` /
        // `*_FILE_IDENTIFIER` / `*_SCHEMA_VERSION`) for the `nmp-core` kernel +
        // actor `*_fb.rs` codecs, from each projection's PROJECTION_CONTRACT
        // entry. Writes one `<name>_producer_consts.generated.rs` per producer
        // under `--repo-root` (default `.`); no stdin.
        "producer-consts" => cli_builtin::run_gen_producer_consts(args, &h),
        // #1493 P9 — generate the native known-signer detection lists (Kotlin
        // `KNOWN_NOSTR_SIGNERS` + Swift `knownSigners`) from the Rust catalog
        // JSON on stdin (`dump_signer_catalog`). Mirrors `gen swift`: reads the
        // catalog from stdin, `--check` diffs the generated files + asserts the
        // AndroidManifest/Info.plist schemes.
        "signer-catalog" => cli::run_gen_signer_catalog(args, &h),
        // NOTE (ADR-0046): `gen modules` was deleted. Composition is explicit
        // app Rust over owner crates, not a generated FFI crate.
        other => Err(format!("unknown subcommand `gen {other}`\n{h}")),
    }
}

fn help() -> String {
    "usage:\n  \
     nmp gen swift             [--schemas - | <path>] --out <path> [--check]\n  \
     nmp gen typed-decoders    --out <path> [--check]\n  \
     nmp gen projection-cache  --platform swift|kotlin --out <path> [--check]\n  \
     nmp gen keyed-ref-cache   --platform swift|kotlin --out <path> [--check]\n  \
     nmp gen action-builders   --platform swift|kotlin|ts [--registry <path>] [--out <path>] [--check]\n  \
     nmp gen action-builders   --registry <path> --check\n  \
     nmp gen action-contract-report [--out <path>]\n  \
     nmp gen builtin-keys      [--out <path>] [--check]\n  \
     nmp gen builtin-deps      [--out <path>] [--check]\n  \
     nmp gen presence-keys     [--out <path>] [--check]\n  \
     nmp gen producer-consts   [--repo-root <path>] [--check]\n  \
     nmp gen signer-catalog    [--catalog - | <path>] [--check]"
        .to_string()
}
