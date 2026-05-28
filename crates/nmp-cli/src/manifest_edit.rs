use std::fs;
use std::path::{Path, PathBuf};

pub fn manifest_arg(args: &[String]) -> Result<PathBuf, String> {
    let mut manifest = PathBuf::from("nmp.toml");
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
            other => return Err(format!("unknown argument {other}")),
        }
        index += 1;
    }
    Ok(manifest)
}

pub fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("reading {}: {error}", path.display()))
}

pub fn write(path: &Path, body: &str) -> Result<(), String> {
    fs::write(path, body).map_err(|error| format!("writing {}: {error}", path.display()))
}

pub fn replace_nmp_section(body: &str, version: &str) -> String {
    let replacement = format!("[nmp]\ndependency_mode = \"version\"\nversion = \"{version}\"\n");
    let mut out = String::new();
    let mut in_nmp = false;
    let mut wrote = false;
    let mut saw_nmp = false;

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_nmp && !wrote {
                out.push_str(&replacement);
                wrote = true;
            }
            in_nmp = trimmed == "[nmp]";
            if in_nmp {
                saw_nmp = true;
                continue;
            }
        }
        if !in_nmp {
            out.push_str(line);
            out.push('\n');
        }
    }

    if in_nmp && !wrote {
        out.push_str(&replacement);
    } else if !saw_nmp {
        let insertion = format!("\n{replacement}");
        if let Some(index) = out.find("\n[modules]") {
            out.insert_str(index + 1, &insertion);
        } else {
            out.push_str(&insertion);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_existing_nmp_section() {
        let body = "[app]\nname = \"x\"\n\n[nmp]\ndependency_mode = \"path\"\npath = \".\"\n\n[modules]\nkernel = \"nmp-core\"\n";
        let next = replace_nmp_section(body, "0.2.0");

        assert!(next.contains("dependency_mode = \"version\""));
        assert!(next.contains("version = \"0.2.0\""));
        assert!(!next.contains("path = \".\""));
        assert!(next.contains("[modules]"));
    }

    #[test]
    fn inserts_nmp_section_before_modules() {
        let body = "[app]\nname = \"x\"\n\n[modules]\nkernel = \"nmp-core\"\n";
        let next = replace_nmp_section(body, "0.2.0");
        let nmp = next.find("[nmp]").unwrap();
        let modules = next.find("[modules]").unwrap();

        assert!(nmp < modules);
    }
}
