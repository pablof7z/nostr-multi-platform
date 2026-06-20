use std::path::{Path, PathBuf};

pub(crate) const USAGE: &str =
    "usage: doctrine-lint [--crate <name>] [--path <dir>] [--allow-findings] \
     [--a6-extra-scope <fragment>] \
     [--d8-extra-scope <fragment>] [--d9-extra-scope <fragment>] \
     [--d10-extra-scope <fragment>] [--d12-extra-scope <fragment>] \
     [--d13-extra-scope <fragment>] [--d14-extra-scope <fragment>] \
     [--d15-extra-scope <fragment>] [--d16-extra-scope <fragment>] \
     [--d17-extra-scope <fragment>] [--d19-extra-scope <fragment>] \
     [--d20-extra-scope <fragment>] [--d21-extra-scope <fragment>] \
     [--d23-extra-scope <fragment>] [--d24-extra-scope <fragment>] \
     [--d25-extra-scope <fragment>] [--d26-extra-scope <fragment>] \
     [--d27-extra-scope <fragment>] \
     [--workspace-d8 [--workspace-d8-root <dir>]] \
     [--workspace-native [--workspace-native-root <dir>]]";

#[derive(Default)]
pub(crate) struct Config {
    pub(crate) crate_name: Option<String>,
    pub(crate) explicit_paths: Vec<PathBuf>,
    pub(crate) allow_findings: bool,
    pub(crate) a6_extra_scopes: Vec<String>,
    pub(crate) d8_extra_scopes: Vec<String>,
    pub(crate) d9_extra_scopes: Vec<String>,
    pub(crate) d10_extra_scopes: Vec<String>,
    pub(crate) d12_extra_scopes: Vec<String>,
    pub(crate) d13_extra_scopes: Vec<String>,
    pub(crate) d14_extra_scopes: Vec<String>,
    pub(crate) d15_extra_scopes: Vec<String>,
    pub(crate) d16_extra_scopes: Vec<String>,
    pub(crate) d17_extra_scopes: Vec<String>,
    pub(crate) d19_extra_scopes: Vec<String>,
    pub(crate) d20_extra_scopes: Vec<String>,
    pub(crate) d21_extra_scopes: Vec<String>,
    pub(crate) d23_extra_scopes: Vec<String>,
    pub(crate) d24_extra_scopes: Vec<String>,
    pub(crate) d25_extra_scopes: Vec<String>,
    pub(crate) d26_extra_scopes: Vec<String>,
    pub(crate) d27_extra_scopes: Vec<String>,
    pub(crate) workspace_d8: bool,
    pub(crate) workspace_d8_root: Option<PathBuf>,
    pub(crate) workspace_native: bool,
    pub(crate) workspace_native_root: Option<PathBuf>,
}

pub(crate) fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut cfg = Config::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--crate" => {
                i += 1;
                cfg.crate_name = Some(required(args, i, "--crate requires a name")?.clone());
            }
            "--path" => {
                i += 1;
                cfg.explicit_paths.push(PathBuf::from(required(
                    args,
                    i,
                    "--path requires a path",
                )?));
            }
            "--allow-findings" => cfg.allow_findings = true,
            "--a6-extra-scope" => push_required(
                &mut cfg.a6_extra_scopes,
                args,
                &mut i,
                "--a6-extra-scope requires a path fragment",
            )?,
            "--d8-extra-scope" => push_required(
                &mut cfg.d8_extra_scopes,
                args,
                &mut i,
                "--d8-extra-scope requires a path fragment",
            )?,
            "--d9-extra-scope" => push_required(
                &mut cfg.d9_extra_scopes,
                args,
                &mut i,
                "--d9-extra-scope requires a path fragment",
            )?,
            "--d10-extra-scope" => push_required(
                &mut cfg.d10_extra_scopes,
                args,
                &mut i,
                "--d10-extra-scope requires a path fragment",
            )?,
            "--d12-extra-scope" => push_required(
                &mut cfg.d12_extra_scopes,
                args,
                &mut i,
                "--d12-extra-scope requires a path fragment",
            )?,
            "--d13-extra-scope" => push_required(
                &mut cfg.d13_extra_scopes,
                args,
                &mut i,
                "--d13-extra-scope requires a path fragment",
            )?,
            "--d14-extra-scope" => push_required(
                &mut cfg.d14_extra_scopes,
                args,
                &mut i,
                "--d14-extra-scope requires a path fragment",
            )?,
            "--d15-extra-scope" => push_required(
                &mut cfg.d15_extra_scopes,
                args,
                &mut i,
                "--d15-extra-scope requires a path fragment",
            )?,
            "--d16-extra-scope" => push_required(
                &mut cfg.d16_extra_scopes,
                args,
                &mut i,
                "--d16-extra-scope requires a path fragment",
            )?,
            "--d17-extra-scope" => push_required(
                &mut cfg.d17_extra_scopes,
                args,
                &mut i,
                "--d17-extra-scope requires a path fragment",
            )?,
            "--d19-extra-scope" => push_required(
                &mut cfg.d19_extra_scopes,
                args,
                &mut i,
                "--d19-extra-scope requires a path fragment",
            )?,
            "--d20-extra-scope" => push_required(
                &mut cfg.d20_extra_scopes,
                args,
                &mut i,
                "--d20-extra-scope requires a path fragment",
            )?,
            "--d21-extra-scope" => push_required(
                &mut cfg.d21_extra_scopes,
                args,
                &mut i,
                "--d21-extra-scope requires a path fragment",
            )?,
            "--d23-extra-scope" => push_required(
                &mut cfg.d23_extra_scopes,
                args,
                &mut i,
                "--d23-extra-scope requires a path fragment",
            )?,
            "--d24-extra-scope" => push_required(
                &mut cfg.d24_extra_scopes,
                args,
                &mut i,
                "--d24-extra-scope requires a path fragment",
            )?,
            "--d25-extra-scope" => push_required(
                &mut cfg.d25_extra_scopes,
                args,
                &mut i,
                "--d25-extra-scope requires a path fragment",
            )?,
            "--d26-extra-scope" => push_required(
                &mut cfg.d26_extra_scopes,
                args,
                &mut i,
                "--d26-extra-scope requires a path fragment",
            )?,
            "--d27-extra-scope" => push_required(
                &mut cfg.d27_extra_scopes,
                args,
                &mut i,
                "--d27-extra-scope requires a path fragment",
            )?,
            "--workspace-d8" => cfg.workspace_d8 = true,
            "--workspace-d8-root" => {
                i += 1;
                cfg.workspace_d8_root = Some(PathBuf::from(required(
                    args,
                    i,
                    "--workspace-d8-root requires a path",
                )?));
            }
            "--workspace-native" => cfg.workspace_native = true,
            "--workspace-native-root" => {
                i += 1;
                cfg.workspace_native_root = Some(PathBuf::from(required(
                    args,
                    i,
                    "--workspace-native-root requires a path",
                )?));
            }
            "-h" | "--help" => {
                println!("{}", USAGE);
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {}", other)),
        }
        i += 1;
    }

    let special_modes = cfg.workspace_d8 as u8 + cfg.workspace_native as u8;
    if special_modes > 1 {
        return Err("--workspace-d8 and --workspace-native cannot be combined".to_string());
    }
    if cfg.workspace_d8 || cfg.workspace_native {
        if cfg.crate_name.is_some() || !cfg.explicit_paths.is_empty() {
            return Err("workspace modes cannot be combined with --crate or --path".to_string());
        }
    } else {
        if cfg.workspace_d8_root.is_some() {
            return Err("--workspace-d8-root requires --workspace-d8".to_string());
        }
        if cfg.workspace_native_root.is_some() {
            return Err("--workspace-native-root requires --workspace-native".to_string());
        }
        if cfg.crate_name.is_none() && cfg.explicit_paths.is_empty() {
            cfg.crate_name = Some("nmp-core".to_string());
        }
    }
    Ok(cfg)
}

fn required<'a>(args: &'a [String], idx: usize, msg: &str) -> Result<&'a String, String> {
    args.get(idx).ok_or_else(|| msg.to_string())
}

fn push_required(
    out: &mut Vec<String>,
    args: &[String],
    idx: &mut usize,
    msg: &str,
) -> Result<(), String> {
    *idx += 1;
    out.push(required(args, *idx, msg)?.clone());
    Ok(())
}

pub(crate) fn resolve_roots(cfg: &Config) -> Result<Vec<PathBuf>, String> {
    if cfg.workspace_d8 {
        let workspace_root = cfg
            .workspace_d8_root
            .clone()
            .unwrap_or_else(default_workspace_root);
        return workspace_crate_src_roots(&workspace_root);
    }
    if cfg.workspace_native {
        return Ok(vec![cfg
            .workspace_native_root
            .clone()
            .unwrap_or_else(default_workspace_root)]);
    }

    let mut roots = Vec::new();
    if let Some(name) = &cfg.crate_name {
        roots.push(PathBuf::from(format!("crates/{}/src", name)));
    }
    roots.extend(cfg.explicit_paths.iter().cloned());
    Ok(roots)
}

/// The workspace root, resolved from CARGO_MANIFEST_DIR so lint modes are
/// independent of the caller's current working directory.
pub(crate) fn default_workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest)
}

const WORKSPACE_D8_SKIP_CRATES: &[&str] = &["nmp-android-ffi", "nmp-testing"];

fn workspace_crate_src_roots(workspace_root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    let crates_dir = workspace_root.join("crates");
    let entries = std::fs::read_dir(&crates_dir)
        .map_err(|e| format!("failed to read {}: {}", crates_dir.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read crates/ entry: {}", e))?;
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if WORKSPACE_D8_SKIP_CRATES.contains(&name.as_ref()) {
            continue;
        }
        let src = entry.path().join("src");
        if src.is_dir() {
            roots.push(src);
        }
    }

    let apps_dir = workspace_root.join("apps");
    if apps_dir.is_dir() {
        let app_entries = std::fs::read_dir(&apps_dir)
            .map_err(|e| format!("failed to read {}: {}", apps_dir.display(), e))?;
        for app_entry in app_entries {
            let app_entry = app_entry.map_err(|e| format!("failed to read apps/ entry: {}", e))?;
            if !app_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let crate_entries = std::fs::read_dir(app_entry.path())
                .map_err(|e| format!("failed to read {}: {}", app_entry.path().display(), e))?;
            for crate_entry in crate_entries {
                let crate_entry =
                    crate_entry.map_err(|e| format!("failed to read app crate entry: {}", e))?;
                if !crate_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let src = crate_entry.path().join("src");
                if src.is_dir() {
                    roots.push(src);
                }
            }
        }
    }

    roots.sort();
    Ok(roots)
}
