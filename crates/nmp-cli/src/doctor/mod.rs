mod checks;
mod model;

use std::env;
use std::path::PathBuf;

use checks::run_checks;
use model::{load_input, Diagnostic, Level};

pub fn run(args: &[String]) -> i32 {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", usage());
        return 0;
    }
    let options = match Options::parse(args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("nmp doctor: {error}");
            eprintln!("{}", usage());
            return 2;
        }
    };
    let input = match load_input(&options.manifest) {
        Ok(input) => input,
        Err(error) => {
            eprintln!("nmp doctor: {error}");
            return 2;
        }
    };
    let mut diagnostics = run_checks(&input);
    if options.strict {
        for diagnostic in &mut diagnostics {
            if diagnostic.level == Level::Warning {
                diagnostic.level = Level::Error;
            }
        }
    }
    if options.json {
        println!("{}", render_json(&diagnostics));
    } else {
        render_human(&diagnostics);
    }
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.level == Level::Error)
    {
        1
    } else {
        0
    }
}

struct Options {
    manifest: PathBuf,
    json: bool,
    strict: bool,
}

impl Options {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut manifest = PathBuf::from("nmp.toml");
        let mut json = false;
        let mut strict = false;
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--json" => json = true,
                "--strict" => strict = true,
                "--manifest" => {
                    index += 1;
                    let Some(path) = args.get(index) else {
                        return Err("--manifest requires a path".to_string());
                    };
                    manifest = PathBuf::from(path);
                }
                other => return Err(format!("unknown option `{other}`")),
            }
            index += 1;
        }
        if manifest.is_relative() {
            manifest = env::current_dir()
                .map_err(|error| error.to_string())?
                .join(manifest);
        }
        Ok(Self {
            manifest,
            json,
            strict,
        })
    }
}

fn usage() -> &'static str {
    "usage: nmp doctor [--json] [--strict] [--manifest nmp.toml]"
}

fn render_human(diagnostics: &[Diagnostic]) {
    for level in [Level::Error, Level::Warning, Level::Info] {
        let bucket: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.level == level)
            .collect();
        if bucket.is_empty() {
            continue;
        }
        println!("{}:", level.as_str());
        for diagnostic in bucket {
            println!(
                "  {} {} - {}",
                diagnostic.id, diagnostic.subject, diagnostic.message
            );
        }
    }
}

fn render_json(diagnostics: &[Diagnostic]) -> String {
    let mut out = String::from("[");
    for (index, diagnostic) in diagnostics.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":\"");
        out.push_str(diagnostic.id);
        out.push_str("\",\"level\":\"");
        out.push_str(diagnostic.level.as_str());
        out.push_str("\",\"message\":\"");
        out.push_str(&escape_json(&diagnostic.message));
        out.push_str("\",\"subject\":\"");
        out.push_str(&escape_json(&diagnostic.subject));
        out.push_str("\"}");
    }
    out.push(']');
    out
}

fn escape_json(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}
