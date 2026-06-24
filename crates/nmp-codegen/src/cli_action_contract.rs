//! #1939 — `nmp gen action-contract-report` CLI handler.
//!
//! Renders the neutral typed action contract as a compact Markdown table for PR
//! review. The report is intentionally generated on demand instead of checked
//! in, so it cannot become a parallel source of truth.

use std::path::PathBuf;

/// `nmp gen action-contract-report [--out <path>]`.
pub fn run_gen_action_contract_report(args: Vec<String>, help: &str) -> Result<(), String> {
    let mut out: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                index += 1;
                out = Some(
                    args.get(index)
                        .map(PathBuf::from)
                        .ok_or_else(|| "--out requires a path".to_string())?,
                );
            }
            other => return Err(format!("unknown argument {other}\n{help}")),
        }
        index += 1;
    }

    let report = nmp_codegen::render_action_contract_report();
    if let Some(path) = out {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, report).map_err(|e| e.to_string())?;
        println!("wrote {}", path.display());
    } else {
        print!("{report}");
    }
    Ok(())
}
