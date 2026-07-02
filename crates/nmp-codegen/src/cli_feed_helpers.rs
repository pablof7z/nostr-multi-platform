//! `nmp gen feed-helpers` CLI handler.

use std::path::PathBuf;

use nmp_codegen::FeedHelperPlatform;

/// `nmp gen feed-helpers --platform swift|kotlin --out <path> [--check]`.
pub fn run_gen_feed_helpers(args: Vec<String>, help: &str) -> Result<(), String> {
    let mut platform_arg: Option<String> = None;
    let mut check = false;
    let mut out: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--platform" => {
                index += 1;
                platform_arg = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--platform requires swift|kotlin".to_string())?,
                );
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
            other => return Err(format!("unknown argument {other}\n{help}")),
        }
        index += 1;
    }

    let platform_arg =
        platform_arg.ok_or_else(|| format!("--platform is required (swift|kotlin)\n{help}"))?;
    let platform = FeedHelperPlatform::parse(&platform_arg).map_err(|e| format!("{e}\n{help}"))?;
    let out = out.ok_or_else(|| "--out is required".to_string())?;

    if check {
        let outcome =
            nmp_codegen::check_feed_helpers(platform, &out).map_err(|err| err.to_string())?;
        if outcome.up_to_date {
            println!(
                "nmp gen feed-helpers --platform {platform_arg} --check: ok ({})",
                out.display()
            );
            return Ok(());
        }
        let where_diff = outcome
            .first_diff_line
            .map(|line| format!(" (first differing line {line})"))
            .unwrap_or_else(|| " (file missing)".to_string());
        Err(format!(
            "feed-helpers ({platform_arg}) codegen stale at {}{where_diff}.\n\
             Regenerate with:\n  \
             cargo run -p nmp-codegen -- gen feed-helpers --platform {platform_arg} --out {}",
            out.display(),
            out.display()
        ))
    } else {
        nmp_codegen::generate_feed_helpers(platform, &out).map_err(|err| err.to_string())?;
        println!("wrote {}", out.display());
        Ok(())
    }
}
