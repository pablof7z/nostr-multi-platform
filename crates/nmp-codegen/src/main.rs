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
        "modules" => run_gen_modules(args),
        // V6 Stage 1 — Swift `Decodable` emitter. Reads a projection schema
        // document (default: stdin) and writes Swift to `--out`. See
        // `crates/nmp-codegen/src/swift.rs` for the emitter itself.
        "swift" => run_gen_swift(args),
        other => Err(format!("unknown subcommand `gen {other}`\n{}", help())),
    }
}

fn run_gen_modules(args: Vec<String>) -> Result<(), String> {
    let mut manifest = PathBuf::from("nmp.toml");
    let mut out = None;
    let mut check = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--manifest" => {
                index += 1;
                manifest = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or_else(|| "--manifest requires a path".to_string())?;
            }
            "--out" => {
                index += 1;
                out = args.get(index).map(PathBuf::from);
            }
            "--check" => check = true,
            other => return Err(format!("unknown argument {other}\n{}", help())),
        }
        index += 1;
    }

    let manifest_model = nmp_codegen::AppManifest::read(&manifest)?;
    let out = out.unwrap_or_else(|| {
        PathBuf::from(format!(
            "apps/{}/{}",
            manifest_model.name,
            nmp_codegen::app_crate_name(&manifest_model.name)
        ))
    });

    if check {
        if nmp_codegen::check_modules(&manifest, &out)? {
            println!("nmp gen modules --check: ok");
            Ok(())
        } else {
            Err("generated module crate is stale".to_string())
        }
    } else {
        let report = nmp_codegen::generate_modules(&manifest, &out)?;
        println!(
            "generated {} for {} ({} files)",
            report.crate_name,
            report.app_name,
            report.files.len()
        );
        Ok(())
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
     nmp gen modules [--manifest nmp.toml] [--out DIR] [--check]\n  \
     nmp gen swift   [--schemas - | <path>] [--out <path>] [--check]"
        .to_string()
}
