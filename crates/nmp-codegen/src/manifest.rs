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

    #[test]
    fn display_name_defaults_to_name_when_omitted() {
        // `[app].display_name` is optional; absent, it falls back to `name`.
        let parsed = AppManifest::parse(
            r#"
            [app]
            name = "chirp"

            [modules]
            kernel = "nmp-core"
            "#,
        )
        .unwrap();
        assert_eq!(parsed.display_name, "chirp");
        // Explicitly set, it is honored verbatim.
        let with_display = AppManifest::parse(
            r#"
            [app]
            name = "chirp"
            display_name = "Chirp App"
            "#,
        )
        .unwrap();
        assert_eq!(with_display.display_name, "Chirp App");
    }

    #[test]
    fn kernel_defaults_to_nmp_core_when_omitted() {
        // The `[modules].kernel` key is optional and defaults to `nmp-core`.
        let parsed = AppManifest::parse(
            r#"
            [app]
            name = "fixture"
            "#,
        )
        .unwrap();
        assert_eq!(parsed.modules.kernel, "nmp-core");
    }

    #[test]
    fn comments_after_a_value_are_stripped() {
        // A `#` begins a comment anywhere on the line, including after a
        // key/value pair. The comment text must not leak into the parsed value.
        let parsed = AppManifest::parse(
            r#"
            [app]
            name = "fixture"   # the app's crate-name stem

            [modules]          # module wiring section
            kernel = "nmp-core"
            "#,
        )
        .unwrap();
        assert_eq!(parsed.name, "fixture", "trailing comment must not be captured");
        assert_eq!(parsed.modules.kernel, "nmp-core");
    }

    #[test]
    fn empty_arrays_parse_to_empty_vecs() {
        // `protocol = []` / `app = []` are valid and yield empty module lists,
        // so `ordered_modules()` is empty — the zero-module codegen path.
        let parsed = AppManifest::parse(
            r#"
            [app]
            name = "bare"

            [modules]
            kernel = "nmp-core"
            protocol = []
            app = []
            "#,
        )
        .unwrap();
        assert!(parsed.modules.protocol.is_empty());
        assert!(parsed.modules.app.is_empty());
        assert!(parsed.ordered_modules().is_empty());
    }

    #[test]
    fn ordered_modules_lists_protocol_before_app() {
        // The ordering contract: every `protocol` module precedes every `app`
        // module, each group in declaration order. Generated enum variants and
        // const lists depend on this being stable.
        let parsed = AppManifest::parse(
            r#"
            [app]
            name = "fixture"

            [modules]
            kernel = "nmp-core"
            protocol = ["nmp-nip01", "nmp-nip22"]
            app = ["fixture-todo-core", "fixture-extra"]
            "#,
        )
        .unwrap();
        assert_eq!(
            parsed.ordered_modules(),
            vec!["nmp-nip01", "nmp-nip22", "fixture-todo-core", "fixture-extra"],
            "protocol modules come first, then app modules, declaration order within each"
        );
    }

    #[test]
    fn missing_name_is_a_typed_error() {
        // `[app].name` is the one required key. Its absence is a recoverable
        // `Err(String)` — never a panic (D6: errors are typed values).
        let err = AppManifest::parse(
            r#"
            [modules]
            kernel = "nmp-core"
            "#,
        )
        .unwrap_err();
        assert!(err.contains("name"), "the error must name the missing key: {err}");
    }

    #[test]
    fn unquoted_string_value_is_rejected() {
        // String values must be double-quoted; a bare token is an error rather
        // than a silently-accepted value.
        let err = AppManifest::parse(
            r#"
            [app]
            name = fixture
            "#,
        )
        .unwrap_err();
        assert!(
            err.contains("quoted string"),
            "unquoted value must report a quoting error: {err}"
        );
    }

    #[test]
    fn line_without_equals_is_rejected() {
        // A non-section, non-comment, non-blank line that has no `=` is
        // malformed and reported, not skipped.
        let err = AppManifest::parse(
            r#"
            [app]
            name = "fixture"
            this line has no equals sign
            "#,
        )
        .unwrap_err();
        assert!(
            err.contains("invalid manifest line"),
            "a line with no `=` must be a typed error: {err}"
        );
    }

    #[test]
    fn unknown_keys_in_known_sections_are_ignored() {
        // Forward-compatibility: an unrecognised key inside a known section is
        // silently ignored (the `_ => {}` catch-all), so adding manifest keys
        // later does not break older parsers.
        let parsed = AppManifest::parse(
            r#"
            [app]
            name = "fixture"
            future_key = "ignored"

            [modules]
            kernel = "nmp-core"
            "#,
        )
        .unwrap();
        assert_eq!(parsed.name, "fixture");
    }
}
