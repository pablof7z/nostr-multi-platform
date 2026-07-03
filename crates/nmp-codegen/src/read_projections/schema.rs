//! Static FlatBuffers schema validation for app-local read projections.

use std::path::{Path, PathBuf};

use super::LoadedAppReadProjectionRegistry;

#[derive(Clone)]
pub struct AppReadProjectionSchema {
    pub key: String,
    pub schema_path: PathBuf,
    pub root_type: String,
    pub file_identifier: String,
    pub schema_version: u32,
    pub schema_id: String,
}

pub fn validate_app_read_projection_schema_files(
    registry_path: &Path,
    loaded: &LoadedAppReadProjectionRegistry,
) -> Result<(), String> {
    let base = registry_path.parent().unwrap_or_else(|| Path::new("."));
    for schema in &loaded.schemas {
        let schema_path = if schema.schema_path.is_absolute() {
            schema.schema_path.clone()
        } else {
            base.join(&schema.schema_path)
        };
        let raw = std::fs::read_to_string(&schema_path).map_err(|e| {
            format!(
                "read schema for projection {:?} at {}: {e}",
                schema.key,
                schema_path.display()
            )
        })?;
        validate_schema_text(schema, &schema_path, &raw)?;
    }
    Ok(())
}

fn validate_schema_text(
    schema: &AppReadProjectionSchema,
    schema_path: &Path,
    raw: &str,
) -> Result<(), String> {
    let lines = strip_line_comments(raw);
    let file_identifier = find_quoted_directive(&lines, "file_identifier");
    if file_identifier.as_deref() != Some(schema.file_identifier.as_str()) {
        return Err(format!(
            "schema {} for projection {:?} declares file_identifier {:?}, expected {:?}",
            schema_path.display(),
            schema.key,
            file_identifier,
            schema.file_identifier
        ));
    }
    let root_type = find_bare_directive(&lines, "root_type");
    if root_type.as_deref() != Some(schema.root_type.as_str()) {
        return Err(format!(
            "schema {} for projection {:?} declares root_type {:?}, expected {:?}",
            schema_path.display(),
            schema.key,
            root_type,
            schema.root_type
        ));
    }
    let root_table = extract_table_body(&lines, &schema.root_type).ok_or_else(|| {
        format!(
            "schema {} for projection {:?} does not declare root table {:?}",
            schema_path.display(),
            schema.key,
            schema.root_type
        )
    })?;
    let field = find_field(&root_table, "schema_version").ok_or_else(|| {
        format!(
            "schema {} root table {:?} must declare schema_version:uint",
            schema_path.display(),
            schema.root_type
        )
    })?;
    if field.ty != "uint" {
        return Err(format!(
            "schema {} root table {:?} declares schema_version:{}, expected uint",
            schema_path.display(),
            schema.root_type,
            field.ty
        ));
    }
    if let Some(default) = field.default {
        let default = default.parse::<u32>().map_err(|_| {
            format!(
                "schema {} root table {:?} has non-integer schema_version default {:?}",
                schema_path.display(),
                schema.root_type,
                default
            )
        })?;
        if default != schema.schema_version {
            return Err(format!(
                "schema {} root table {:?} has schema_version default {}, expected {}",
                schema_path.display(),
                schema.root_type,
                default,
                schema.schema_version
            ));
        }
    }
    Ok(())
}

struct ParsedField<'a> {
    ty: &'a str,
    default: Option<&'a str>,
}

fn strip_line_comments(raw: &str) -> Vec<String> {
    raw.lines()
        .map(|line| line.split_once("//").map_or(line, |(head, _)| head).trim())
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn find_quoted_directive(lines: &[String], directive: &str) -> Option<String> {
    lines.iter().find_map(|line| {
        let rest = line.strip_prefix(directive)?.trim();
        let rest = rest.strip_prefix('"')?;
        let (value, _) = rest.split_once('"')?;
        Some(value.to_string())
    })
}

fn find_bare_directive(lines: &[String], directive: &str) -> Option<String> {
    lines.iter().find_map(|line| {
        let rest = line.strip_prefix(directive)?.trim();
        let rest = rest.strip_suffix(';')?.trim();
        (!rest.is_empty()).then(|| rest.to_string())
    })
}

fn extract_table_body(lines: &[String], table: &str) -> Option<String> {
    let mut body = String::new();
    let mut in_table = false;
    let mut depth: i32 = 0;
    let prefix = format!("table {table}");
    for line in lines {
        if !in_table {
            if line.starts_with(&prefix) {
                in_table = true;
                depth += line.matches('{').count() as i32;
                depth -= line.matches('}').count() as i32;
                if let Some((_, after)) = line.split_once('{') {
                    body.push_str(after);
                    body.push('\n');
                }
                if depth <= 0 {
                    return Some(body);
                }
            }
            continue;
        }
        depth += line.matches('{').count() as i32;
        depth -= line.matches('}').count() as i32;
        if depth < 0 {
            return Some(body);
        }
        body.push_str(line);
        body.push('\n');
        if depth <= 0 {
            return Some(body);
        }
    }
    None
}

fn find_field<'a>(table_body: &'a str, name: &str) -> Option<ParsedField<'a>> {
    for field in table_body.split(';') {
        let field = field.trim();
        let rest = field.strip_prefix(name)?.trim();
        let rest = rest.strip_prefix(':')?.trim();
        let (ty, default) = match rest.split_once('=') {
            Some((ty, default)) => (ty.trim(), Some(default.trim())),
            None => (rest.trim(), None),
        };
        let ty = ty.split_whitespace().next().unwrap_or(ty);
        return Some(ParsedField { ty, default });
    }
    None
}
