use std::env;
use std::io::Read;
use std::path::PathBuf;

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
    match subcommand.as_str() {
        // V6 Stage 1 — Swift `Decodable` emitter. Reads a projection schema
        // document (default: stdin) and writes Swift to `--out`. See
        // `crates/nmp-codegen/src/swift.rs` for the emitter itself.
        "swift" => run_gen_swift(args),
        // V6 Stage 4 (consumer-side) — generated typed-FlatBuffer-sidecar
        // decoders. Writes `TypedProjectionDecoders.generated.swift` from the
        // registry's `typed_sidecar` metadata; no schema-document stdin needed.
        "typed-decoders" => run_gen_typed_decoders(args),
        // ADR-0055 R3-S3 — generated `ProjectionMergeCache` for iOS. Writes
        // `ProjectionCache.generated.swift` from the same registry as
        // `typed-decoders`; implements the D3-3 merge algorithm.
        "projection-cache" => run_gen_projection_cache(args),
        // ADR-0053 / Workstream-E4 — generated `KERNEL_BUILTIN_PROJECTION_KEYS`
        // Rust const for `nmp-core`. Writes
        // `crates/nmp-core/src/kernel/update/builtin_projection_keys.generated.rs`
        // from the SAME projection registry as `typed-decoders`; no stdin.
        "builtin-keys" => run_gen_builtin_keys(args),
        // #1493 P9 — generate the native known-signer detection lists (Kotlin
        // `KNOWN_NOSTR_SIGNERS` + Swift `knownSigners`) from the Rust catalog
        // JSON on stdin (`dump_signer_catalog`). Mirrors `gen swift`: reads the
        // catalog from stdin, `--check` diffs the generated files + asserts the
        // AndroidManifest/Info.plist schemes.
        "signer-catalog" => run_gen_signer_catalog(args),
        // NOTE (ADR-0046): `gen modules` was deleted. Composition is a library
        // (`nmp-defaults::register_defaults`), not a generated FFI crate.
        other => Err(format!("unknown subcommand `gen {other}`\n{}", help())),
    }
}

/// `nmp gen swift [--schemas <path>] [--out <path>] [--check]`.
///
/// `--schemas` defaults to `-` (stdin). The expected input is whatever
/// `dump_projection_schemas` writes (see
/// `crates/nmp-core/src/bin/dump_projection_schemas.rs`).
///
/// `--out` defaults to
/// `ios/Chirp/Chirp/Bridge/Generated/KernelTypes.generated.swift` —
/// matches plan §5b and the xcodegen-swept `Chirp/` source root, so
/// dropping the file in this location picks it up on the next project
/// regeneration without a pbxproj edit (xcodegen `sources: - path: Chirp`).
///
/// `--check` diffs against the file on disk and exits non-zero on drift.
/// The CI gate at `.github/workflows/codegen-drift.yml` uses this mode.
fn run_gen_swift(args: Vec<String>) -> Result<(), String> {
    let mut schemas_path = PathBuf::from("-");
    let mut out = PathBuf::from("ios/Chirp/Chirp/Bridge/Generated/KernelTypes.generated.swift");
    let mut check = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--schemas" => {
                index += 1;
                schemas_path = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or_else(|| "--schemas requires a path or `-`".to_string())?;
            }
            "--out" => {
                index += 1;
                out = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or_else(|| "--out requires a path".to_string())?;
            }
            "--check" => check = true,
            other => return Err(format!("unknown argument {other}\n{}", help())),
        }
        index += 1;
    }

    let json = read_schemas(&schemas_path)?;

    if check {
        let outcome = nmp_codegen::check_swift(&json, &out).map_err(|e| e.to_string())?;
        if outcome.up_to_date {
            println!("nmp gen swift --check: ok ({})", out.display());
            Ok(())
        } else {
            let where_diff = outcome
                .first_diff_line
                .map(|n| format!(" (first differing line {n})"))
                .unwrap_or_else(|| " (file missing)".to_string());
            Err(format!(
                "Swift codegen stale at {}{where_diff}.\n\
                 Regenerate with:\n  \
                 cargo run -p nmp-core --features codegen-schema \
                 --bin dump_projection_schemas \
                 | cargo run -p nmp-codegen -- gen swift",
                out.display()
            ))
        }
    } else {
        nmp_codegen::generate_swift(&json, &out).map_err(|e| e.to_string())?;
        println!("wrote {}", out.display());
        Ok(())
    }
}

/// `nmp gen typed-decoders [--out <path>] [--check]`.
///
/// Generates `TypedProjectionDecoders.generated.swift` — the per-projection
/// typed-FlatBuffer-sidecar decoders (consumer side). Driven entirely by the
/// registry's `typed_sidecar` metadata in
/// `crates/nmp-codegen/src/swift_projections_registry.rs`; takes no schema
/// stdin.
///
/// `--out` defaults to
/// `ios/Chirp/Chirp/Bridge/Generated/TypedProjectionDecoders.generated.swift`
/// (alongside `KernelTypes.generated.swift`, picked up by the xcodegen
/// `sources: - path: Chirp` sweep without a pbxproj edit).
///
/// `--check` diffs against the file on disk and exits non-zero on drift. The
/// CI gate at `.github/workflows/codegen-drift.yml` uses this mode.
fn run_gen_typed_decoders(args: Vec<String>) -> Result<(), String> {
    let mut out =
        PathBuf::from("ios/Chirp/Chirp/Bridge/Generated/TypedProjectionDecoders.generated.swift");
    let mut check = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                index += 1;
                out = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or_else(|| "--out requires a path".to_string())?;
            }
            "--check" => check = true,
            other => return Err(format!("unknown argument {other}\n{}", help())),
        }
        index += 1;
    }

    if check {
        let outcome = nmp_codegen::check_typed_decoders(&out).map_err(|e| e.to_string())?;
        if outcome.up_to_date {
            println!("nmp gen typed-decoders --check: ok ({})", out.display());
            Ok(())
        } else {
            let where_diff = outcome
                .first_diff_line
                .map(|n| format!(" (first differing line {n})"))
                .unwrap_or_else(|| " (file missing)".to_string());
            Err(format!(
                "typed-decoder codegen stale at {}{where_diff}.\n\
                 Regenerate with:\n  \
                 cargo run -p nmp-codegen -- gen typed-decoders",
                out.display()
            ))
        }
    } else {
        nmp_codegen::generate_typed_decoders(&out).map_err(|e| e.to_string())?;
        println!("wrote {}", out.display());
        Ok(())
    }
}

/// `nmp gen projection-cache [--platform swift|kotlin] [--out <path>] [--check]`.
///
/// Generates the NMP-owned rev-aware projection cache implementing the
/// ADR-0055 D3-3 merge algorithm. Driven by the same registry as
/// `typed-decoders`; takes no schema stdin.
///
/// `--platform swift` (default): generates
/// `ios/Chirp/Chirp/Bridge/Generated/ProjectionCache.generated.swift`.
///
/// `--platform kotlin`: generates
/// `android/app/src/main/java/org/nmp/android/ProjectionCache.kt`.
///
/// `--check` diffs against the file on disk and exits non-zero on drift. The
/// CI gate at `.github/workflows/codegen-drift.yml` uses this mode.
fn run_gen_projection_cache(args: Vec<String>) -> Result<(), String> {
    let default_swift =
        PathBuf::from("ios/Chirp/Chirp/Bridge/Generated/ProjectionCache.generated.swift");
    let default_kotlin =
        PathBuf::from("android/app/src/main/java/org/nmp/android/ProjectionCache.kt");
    let mut platform = "swift".to_string();
    let mut check = false;
    let mut custom_out: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--platform" => {
                index += 1;
                platform = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--platform requires swift|kotlin".to_string())?;
            }
            "--out" => {
                index += 1;
                custom_out = Some(
                    args.get(index)
                        .map(PathBuf::from)
                        .ok_or_else(|| "--out requires a path".to_string())?,
                );
            }
            "--check" => check = true,
            other => return Err(format!("unknown argument {other}\n{}", help())),
        }
        index += 1;
    }

    match platform.as_str() {
        "swift" => {
            let out = custom_out.unwrap_or(default_swift);
            if check {
                let outcome =
                    nmp_codegen::check_projection_cache(&out).map_err(|e| e.to_string())?;
                if outcome.up_to_date {
                    println!("nmp gen projection-cache --check: ok ({})", out.display());
                    Ok(())
                } else {
                    let where_diff = outcome
                        .first_diff_line
                        .map(|n| format!(" (first differing line {n})"))
                        .unwrap_or_else(|| " (file missing)".to_string());
                    Err(format!(
                        "projection-cache (swift) codegen stale at {}{where_diff}.\n\
                         Regenerate with:\n  \
                         cargo run -p nmp-codegen -- gen projection-cache",
                        out.display()
                    ))
                }
            } else {
                nmp_codegen::generate_projection_cache(&out).map_err(|e| e.to_string())?;
                println!("wrote {}", out.display());
                Ok(())
            }
        }
        "kotlin" => {
            let out = custom_out.unwrap_or(default_kotlin);
            if check {
                let outcome = nmp_codegen::check_kotlin_projection_cache(&out)
                    .map_err(|e| e.to_string())?;
                if outcome.up_to_date {
                    println!(
                        "nmp gen projection-cache --platform kotlin --check: ok ({})",
                        out.display()
                    );
                    Ok(())
                } else {
                    let where_diff = outcome
                        .first_diff_line
                        .map(|n| format!(" (first differing line {n})"))
                        .unwrap_or_else(|| " (file missing)".to_string());
                    Err(format!(
                        "projection-cache (kotlin) codegen stale at {}{where_diff}.\n\
                         Regenerate with:\n  \
                         cargo run -p nmp-codegen -- gen projection-cache --platform kotlin",
                        out.display()
                    ))
                }
            } else {
                nmp_codegen::generate_kotlin_projection_cache(&out)
                    .map_err(|e| e.to_string())?;
                println!("wrote {}", out.display());
                Ok(())
            }
        }
        other => Err(format!(
            "unknown --platform {:?}: expected swift or kotlin\n{}",
            other,
            help()
        )),
    }
}

/// `nmp gen builtin-keys [--out <path>] [--check]`.
///
/// Generates `KERNEL_BUILTIN_PROJECTION_KEYS` — the Tier-2 kernel-owned built-in
/// projection key const `nmp-core` `include!`s. Driven entirely by the projection
/// registry (`swift_projections_registry::kernel_builtin_projection_keys`); takes
/// no schema stdin.
///
/// `--out` defaults to
/// `crates/nmp-core/src/kernel/update/builtin_projection_keys.generated.rs`.
///
/// `--check` diffs against the file on disk and exits non-zero on drift. The CI
/// gate at `.github/workflows/codegen-drift.yml` uses this mode.
fn run_gen_builtin_keys(args: Vec<String>) -> Result<(), String> {
    let mut out =
        PathBuf::from("crates/nmp-core/src/kernel/update/builtin_projection_keys.generated.rs");
    let mut check = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                index += 1;
                out = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or_else(|| "--out requires a path".to_string())?;
            }
            "--check" => check = true,
            other => return Err(format!("unknown argument {other}\n{}", help())),
        }
        index += 1;
    }

    if check {
        let outcome = nmp_codegen::check_builtin_keys(&out).map_err(|e| e.to_string())?;
        if outcome.up_to_date {
            println!("nmp gen builtin-keys --check: ok ({})", out.display());
            Ok(())
        } else {
            let where_diff = outcome
                .first_diff_line
                .map(|n| format!(" (first differing line {n})"))
                .unwrap_or_else(|| " (file missing)".to_string());
            Err(format!(
                "builtin-keys codegen stale at {}{where_diff}.\n\
                 Regenerate with:\n  \
                 cargo run -p nmp-codegen -- gen builtin-keys",
                out.display()
            ))
        }
    } else {
        nmp_codegen::generate_builtin_keys(&out).map_err(|e| e.to_string())?;
        println!("wrote {}", out.display());
        Ok(())
    }
}

/// `nmp gen signer-catalog [--catalog - | <path>] [--check]`.
///
/// `--catalog` defaults to `-` (stdin). The expected input is whatever
/// `dump_signer_catalog` writes (`cargo run -p nmp-core --bin
/// dump_signer_catalog`) — a top-level JSON array of known signer apps.
///
/// Without `--check`, writes the generated native lists (three Kotlin
/// `KnownSigners.generated.kt` copies + two Swift `KnownSigners.generated.swift`
/// copies). With `--check`, diffs each against a fresh render AND asserts the
/// `AndroidManifest <queries>` / `Info.plist LSApplicationQueriesSchemes`
/// schemes match the catalog, exiting non-zero on any drift. The CI gate at
/// `.github/workflows/codegen-drift.yml` uses `--check`.
fn run_gen_signer_catalog(args: Vec<String>) -> Result<(), String> {
    let mut catalog_path = PathBuf::from("-");
    let mut check = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--catalog" => {
                index += 1;
                catalog_path = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or_else(|| "--catalog requires a path or `-`".to_string())?;
            }
            "--check" => check = true,
            other => return Err(format!("unknown argument {other}\n{}", help())),
        }
        index += 1;
    }

    let json = read_schemas(&catalog_path)?;

    if check {
        let outcome = nmp_codegen::check_signer_catalog(&json)?;
        if outcome.up_to_date {
            println!("nmp gen signer-catalog --check: ok");
            Ok(())
        } else {
            Err(format!(
                "signer-catalog codegen stale / drifted:\n  {}\n\
                 Regenerate with:\n  \
                 cargo run -q -p nmp-core --bin dump_signer_catalog \
                 | cargo run -q -p nmp-codegen -- gen signer-catalog\n\
                 (manifest/plist scheme mismatches must be fixed by hand to match the catalog)",
                outcome.problems.join("\n  ")
            ))
        }
    } else {
        let written = nmp_codegen::generate_signer_catalog(&json)?;
        for path in written {
            println!("wrote {}", path.display());
        }
        Ok(())
    }
}

/// Read the schema JSON from `path` (or stdin if `path == "-"`).
fn read_schemas(path: &std::path::Path) -> Result<String, String> {
    if path == std::path::Path::new("-") {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| format!("reading stdin: {e}"))?;
        if s.trim().is_empty() {
            return Err(
                "no schema input on stdin. Pipe `dump_projection_schemas` output, or pass \
                 --schemas <path>."
                    .to_string(),
            );
        }
        Ok(s)
    } else {
        std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))
    }
}

fn help() -> String {
    "usage:\n  \
     nmp gen swift             [--schemas - | <path>] [--out <path>] [--check]\n  \
     nmp gen typed-decoders    [--out <path>] [--check]\n  \
     nmp gen projection-cache  [--platform swift|kotlin] [--out <path>] [--check]\n  \
     nmp gen builtin-keys      [--out <path>] [--check]\n  \
     nmp gen signer-catalog    [--catalog - | <path>] [--check]"
        .to_string()
}
