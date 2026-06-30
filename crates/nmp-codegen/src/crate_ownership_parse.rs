use std::fs;
use std::path::{Path, PathBuf};

use crate::crate_ownership::{OwnershipClaim, OwnershipDescriptor, OwnershipNote};

pub(super) fn descriptor_for_package(
    package_name: &str,
    manifest_path: &Path,
) -> Result<Option<OwnershipDescriptor>, String> {
    let root = manifest_path
        .parent()
        .ok_or_else(|| "manifest has no parent directory".to_string())?;
    let mut matches = Vec::new();
    for path in rust_files(&root.join("src"))? {
        let source = fs::read_to_string(&path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        if let Some(body) = extract_macro_body(&source)? {
            matches.push((path, body));
        }
    }
    match matches.len() {
        0 => Ok(None),
        1 => parse_descriptor(&matches[0].1, matches[0].0.clone()).map(Some),
        _ => Err(format!(
            "{package_name} has multiple declare_crate_ownership! descriptors"
        )),
    }
}

fn parse_descriptor(body: &str, source_path: PathBuf) -> Result<OwnershipDescriptor, String> {
    let owner_id = string_field(body, "owner_id")?;
    let crate_name = string_field(body, "crate_name")?;
    let summary = string_field(body, "summary")?;
    let claims_body = block_field(body, "claims", '[', ']')?;
    let notes_body = block_field(body, "notes", '[', ']')?;
    let claims = top_level_objects(&claims_body)
        .into_iter()
        .map(|claim| parse_claim(&claim))
        .collect::<Result<Vec<_>, _>>()?;
    let notes = top_level_objects(&notes_body)
        .into_iter()
        .map(|note| {
            Ok(OwnershipNote {
                claim: string_field(&note, "claim")?,
                text: string_field(&note, "text")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(OwnershipDescriptor {
        owner_id,
        crate_name,
        summary,
        claims,
        notes,
        source_path,
    })
}

fn parse_claim(body: &str) -> Result<OwnershipClaim, String> {
    let scope = block_field(body, "scope", '{', '}')?;
    Ok(OwnershipClaim {
        claim_type: string_field(body, "claim_type")?,
        id: string_field(body, "id")?,
        exclusive: bool_field(body, "exclusive")?,
        scope_kind: string_field(&scope, "kind")?,
        scope_value: string_field(&scope, "value")?,
        context: string_field(&scope, "context")?,
        owns: string_array_field(body, "owns")?,
    })
}

fn rust_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in
        fs::read_dir(dir).map_err(|err| format!("failed to read {}: {err}", dir.display()))?
    {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.is_dir() {
            out.extend(rust_files(&path)?);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(out)
}

fn extract_macro_body(source: &str) -> Result<Option<String>, String> {
    let mut search_from = 0;
    while let Some(relative) = source[search_from..].find("declare_crate_ownership!") {
        let index = search_from + relative;
        let after_name = index + "declare_crate_ownership!".len();
        let rest = source[after_name..].trim_start();
        if rest.starts_with('{') {
            let open = source.len() - rest.len();
            let close = matching_delimiter(source, open, '{', '}')?;
            return Ok(Some(source[open + 1..close].to_string()));
        }
        search_from = after_name;
    }
    Ok(None)
}

fn string_field(source: &str, key: &str) -> Result<String, String> {
    let value = value_after_key(source, key)?;
    parse_string(value).map(|(s, _)| s)
}

fn bool_field(source: &str, key: &str) -> Result<bool, String> {
    let value = value_after_key(source, key)?;
    if value.starts_with("true") {
        Ok(true)
    } else if value.starts_with("false") {
        Ok(false)
    } else {
        Err(format!("field {key} must be true or false"))
    }
}

fn block_field(source: &str, key: &str, open: char, close: char) -> Result<String, String> {
    let value = value_after_key(source, key)?;
    let offset = source.len() - value.len();
    let open_offset = value
        .find(open)
        .ok_or_else(|| format!("field {key} must start with {open}"))?;
    let start = offset + open_offset;
    let end = matching_delimiter(source, start, open, close)?;
    Ok(source[start + 1..end].to_string())
}

fn string_array_field(source: &str, key: &str) -> Result<Vec<String>, String> {
    let body = block_field(source, key, '[', ']')?;
    let mut rest = body.as_str();
    let mut out = Vec::new();
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        if rest.is_empty() {
            break;
        }
        let (value, next) = parse_string(rest)?;
        out.push(value);
        rest = &rest[next..];
    }
    Ok(out)
}

fn value_after_key<'a>(source: &'a str, key: &str) -> Result<&'a str, String> {
    let needle = format!("{key}:");
    let index = source
        .find(&needle)
        .ok_or_else(|| format!("missing field {key}"))?;
    Ok(source[index + needle.len()..].trim_start())
}

fn parse_string(source: &str) -> Result<(String, usize), String> {
    let mut chars = source.char_indices();
    if chars.next().map(|(_, c)| c) != Some('"') {
        return Err("expected string literal".to_string());
    }
    let mut escaped = false;
    let mut out = String::new();
    for (index, ch) in chars {
        if escaped {
            out.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Ok((out, index + ch.len_utf8()));
        } else {
            out.push(ch);
        }
    }
    Err("unterminated string literal".to_string())
}

fn top_level_objects(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in source.char_indices() {
        if in_string {
            escaped = !escaped && ch == '\\';
            if !escaped && ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(index);
                }
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some(start) = start.take() {
                        out.push(source[start + 1..index].to_string());
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn matching_delimiter(
    source: &str,
    start: usize,
    open: char,
    close: char,
) -> Result<usize, String> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in source[start..].char_indices() {
        let absolute = start + index;
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            c if c == open => depth += 1,
            c if c == close => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Ok(absolute);
                }
            }
            _ => {}
        }
    }
    Err(format!("missing closing delimiter {close}"))
}
