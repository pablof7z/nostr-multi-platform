use std::env;
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
    if args.len() < 2 || args[0] != "gen" || args[1] != "modules" {
        return Err(help());
    }
    args.drain(0..2);

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

fn help() -> String {
    "usage: nmp gen modules [--manifest nmp.toml] [--out DIR] [--check]".to_string()
}
