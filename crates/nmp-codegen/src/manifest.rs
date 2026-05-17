use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppManifest {
    pub name: String,
    pub display_name: String,
    pub modules: ModuleSet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleSet {
    pub kernel: String,
    pub protocol: Vec<String>,
    pub app: Vec<String>,
}

impl AppManifest {
    pub fn read(path: &Path) -> Result<Self, String> {
        Self::parse(&fs::read_to_string(path).map_err(|error| error.to_string())?)
    }

    pub fn parse(input: &str) -> Result<Self, String> {
        let mut section = "";
        let mut name = None;
        let mut display_name = None;
        let mut kernel = None;
        let mut protocol = Vec::new();
        let mut app = Vec::new();

        for raw_line in input.lines() {
            let line = raw_line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = &line[1..line.len() - 1];
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("invalid manifest line: {line}"));
            };
            let key = key.trim();
            let value = value.trim();
            match (section, key) {
                ("app", "name") => name = Some(parse_string(value)?),
                ("app", "display_name") => display_name = Some(parse_string(value)?),
                ("modules", "kernel") => kernel = Some(parse_string(value)?),
                ("modules", "protocol") => protocol = parse_array(value)?,
                ("modules", "app") => app = parse_array(value)?,
                _ => {}
            }
        }

        let name = name.ok_or_else(|| "missing [app].name".to_string())?;
        Ok(Self {
            display_name: display_name.clone().unwrap_or_else(|| name.clone()),
            name,
            modules: ModuleSet {
                kernel: kernel.unwrap_or_else(|| "nmp-core".to_string()),
                protocol,
                app,
            },
        })
    }

    pub fn ordered_modules(&self) -> Vec<String> {
        self.modules
            .protocol
            .iter()
            .chain(self.modules.app.iter())
            .cloned()
            .collect()
    }
}

fn parse_string(value: &str) -> Result<String, String> {
    value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("expected quoted string, got {value}"))
}

fn parse_array(value: &str) -> Result<Vec<String>, String> {
    let inner = value
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| format!("expected array, got {value}"))?;
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    inner
        .split(',')
        .map(|part| parse_string(part.trim()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_manifest() {
        let parsed = AppManifest::parse(
            r#"
            [app]
            name = "fixture"

            [modules]
            kernel = "nmp-core"
            protocol = ["nmp-nip01"]
            app = ["fixture-todo-core"]
            "#,
        )
        .unwrap();

        assert_eq!(parsed.name, "fixture");
        assert_eq!(parsed.modules.kernel, "nmp-core");
        assert_eq!(
            parsed.ordered_modules(),
            vec!["nmp-nip01", "fixture-todo-core"]
        );
    }
}
