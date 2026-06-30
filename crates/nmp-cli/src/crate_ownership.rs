use nmp_codegen::{
    load_workspace_ownership, render_ownership_human, render_ownership_json, render_ownership_tsv,
    OwnershipQuery,
};
use std::path::PathBuf;

pub fn run(args: &[String]) -> Result<(), String> {
    let mut audit = false;
    let mut deny = false;
    let mut format = Format::Human;
    let mut query = OwnershipQuery::default();
    let mut manifest_path: Option<PathBuf> = None;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "audit" => {
                audit = true;
                index += 1;
            }
            "--deny" => {
                deny = true;
                index += 1;
            }
            "--format" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "--format requires human, tsv, or json".to_string())?;
                format = Format::parse(value)?;
                index += 2;
            }
            "--crate" => {
                query.crate_filter = Some(value_after(args, index, "--crate")?.to_string());
                index += 2;
            }
            "--scope" => {
                query.scope_kind = Some(value_after(args, index, "--scope")?.to_string());
                index += 2;
            }
            "--value" => {
                query.scope_value = Some(value_after(args, index, "--value")?.to_string());
                index += 2;
            }
            "--manifest-path" => {
                manifest_path = Some(PathBuf::from(value_after(args, index, "--manifest-path")?));
                index += 2;
            }
            "--help" | "-h" => {
                println!("{}", help());
                return Ok(());
            }
            other => {
                return Err(format!(
                    "unknown crate-ownership option `{other}`\n\n{}",
                    help()
                ))
            }
        }
    }

    let workspace = load_workspace_ownership(manifest_path.as_deref())?;
    if audit {
        if workspace.audit_issues.is_empty() {
            println!(
                "ownership audit passed: {} crates, {} claims",
                workspace.descriptors.len(),
                workspace
                    .descriptors
                    .iter()
                    .map(|descriptor| descriptor.claims.len())
                    .sum::<usize>()
            );
            return Ok(());
        }
        for issue in &workspace.audit_issues {
            eprintln!("error[{}]: {}", issue.code, issue.message);
        }
        if deny {
            return Err(format!(
                "ownership audit failed with {} issue(s)",
                workspace.audit_issues.len()
            ));
        }
        return Ok(());
    }

    match format {
        Format::Human => print!("{}", render_ownership_human(&workspace, &query)),
        Format::Tsv => print!("{}", render_ownership_tsv(&workspace, &query)),
        Format::Json => println!("{}", render_ownership_json(&workspace)?),
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Human,
    Tsv,
    Json,
}

impl Format {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "human" => Ok(Self::Human),
            "tsv" => Ok(Self::Tsv),
            "json" => Ok(Self::Json),
            other => Err(format!(
                "unknown format `{other}`; expected human, tsv, or json"
            )),
        }
    }
}

fn value_after<'a>(args: &'a [String], index: usize, flag: &str) -> Result<&'a str, String> {
    args.get(index + 1)
        .map(String::as_str)
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn help() -> String {
    [
        "usage:",
        "  nmp crate-ownership [--format human|tsv|json]",
        "      Report positive ownership descriptors for active workspace crates.",
        "",
        "  nmp crate-ownership --crate NAME [--format tsv]",
        "      Report one crate's summary, claims, and notes.",
        "",
        "  nmp crate-ownership --scope kind --value 7 --format tsv",
        "      Report claims for one scoped surface.",
        "",
        "  nmp crate-ownership audit [--deny]",
        "      Audit every active workspace crate for a descriptor and fail on",
        "      duplicate exclusive ownership scopes. --deny exits non-zero.",
        "",
        "  nmp crate-ownership --manifest-path Cargo.toml",
        "      Run against another NMP app/workspace manifest.",
    ]
    .join("\n")
}
