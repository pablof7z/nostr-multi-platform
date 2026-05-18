//! `nmp gen modules` — thin front-end over the `nmp-codegen` library.
//!
//! Behaviour mirrors the legacy `nmp` binary shipped inside `nmp-codegen`
//! (same flags, same defaults) so a scaffolded app can run either. We call
//! `nmp_codegen` strictly as a library; the pipeline itself is unmodified.

use nmp_codegen::{app_crate_name, check_modules, generate_modules, AppManifest};
use std::path::PathBuf;

pub fn run(args: &[String]) -> Result<(), String> {
    if args.first().map(String::as_str) != Some("modules") {
        return Err(format!("usage: {USAGE}"));
    }

    let mut manifest = PathBuf::from("nmp.toml");
    let mut out: Option<PathBuf> = None;
    let mut check = false;
    let mut index = 1;
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
                out = Some(
                    args.get(index)
                        .map(PathBuf::from)
                        .ok_or_else(|| "--out requires a path".to_string())?,
                );
            }
            "--check" => check = true,
            other => return Err(format!("unknown argument {other}\nusage: {USAGE}")),
        }
        index += 1;
    }

    let model = AppManifest::read(&manifest)?;
    let out = out.unwrap_or_else(|| {
        PathBuf::from(format!(
            "apps/{}/{}",
            model.name,
            app_crate_name(&model.name)
        ))
    });

    if check {
        if check_modules(&manifest, &out)? {
            println!("nmp gen modules --check: ok");
            Ok(())
        } else {
            Err("generated module crate is stale; run `nmp gen modules`".to_string())
        }
    } else {
        let report = generate_modules(&manifest, &out)?;
        println!(
            "generated {} for {} ({} files) -> {}",
            report.crate_name,
            report.app_name,
            report.files.len(),
            out.display()
        );
        Ok(())
    }
}

const USAGE: &str = "nmp gen modules [--manifest nmp.toml] [--out DIR] [--check]";
