//! Build the root Android content-gallery bundle in the canonical wire shape.
//!
//! Usage: `cargo run -p nmp-content-fixtures --bin build-android-gallery-bundle`
//! from the workspace root.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use nmp_content::{tokenize_with_kind, ContentTreeWire, RenderMode};
use nmp_content_fixtures::{
    build_bundle,
    dto::{ArticleHeaderDto, EmbedEntry, ListDto, ScenarioDto, SignedEventJson},
};
use serde::Serialize;

const ANDROID_BUNDLE_PATH: &str = "android/gallery/src/main/assets/content-gallery-bundle.json";
const ANDROID_BUNDLE_VERSION: u32 = 2;

#[derive(Serialize)]
struct WireBundle {
    version: u32,
    scenarios: Vec<WireScenario>,
}

#[derive(Serialize)]
struct WireScenario {
    id: String,
    category: String,
    title: String,
    exercises: String,
    events: Vec<SignedEventJson>,
    rendered: ContentTreeWire,
    embeds: BTreeMap<String, WireEmbedEntry>,
}

#[derive(Serialize)]
struct WireEmbedEntry {
    resolved_kind: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile_picture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<SignedEventJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rendered: Option<ContentTreeWire>,
    collapsed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    collapse_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    article: Option<ArticleHeaderDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    list: Option<ListDto>,
}

fn main() -> ExitCode {
    match run() {
        Ok(count) => {
            println!("wrote {count} scenarios -> {ANDROID_BUNDLE_PATH}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<usize, String> {
    let source = build_bundle();
    let scenarios = source
        .scenarios
        .into_iter()
        .map(convert_scenario)
        .collect::<Result<Vec<_>, _>>()?;
    let count = scenarios.len();
    let bundle = WireBundle {
        version: ANDROID_BUNDLE_VERSION,
        scenarios,
    };
    let json = serde_json::to_string_pretty(&bundle)
        .map_err(|err| format!("serialize Android gallery bundle failed: {err}"))?;

    let path = Path::new(ANDROID_BUNDLE_PATH);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create {} failed: {err}", parent.display()))?;
    }
    fs::write(path, format!("{json}\n"))
        .map_err(|err| format!("write {ANDROID_BUNDLE_PATH} failed: {err}"))?;
    Ok(count)
}

fn convert_scenario(scenario: ScenarioDto) -> Result<WireScenario, String> {
    let primary = scenario
        .events
        .first()
        .ok_or_else(|| format!("scenario {} has no primary event", scenario.id))?;
    let rendered = wire_for_event(primary);
    let embeds = scenario
        .embeds
        .into_iter()
        .map(|(uri, entry)| {
            let converted = convert_embed(&uri, entry)?;
            Ok((uri, converted))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    Ok(WireScenario {
        id: scenario.id,
        category: scenario.category,
        title: scenario.title,
        exercises: scenario.exercises,
        events: scenario.events,
        rendered,
        embeds,
    })
}

fn convert_embed(uri: &str, entry: EmbedEntry) -> Result<WireEmbedEntry, String> {
    let rendered = if entry.rendered.is_some() {
        let event = entry
            .event
            .as_ref()
            .ok_or_else(|| format!("embed {uri} has rendered content without an event"))?;
        Some(wire_for_event(event))
    } else {
        None
    };
    Ok(WireEmbedEntry {
        resolved_kind: entry.resolved_kind,
        profile_name: entry.profile_name,
        profile_picture: entry.profile_picture,
        event: entry.event,
        rendered,
        collapsed: entry.collapsed,
        collapse_reason: entry.collapse_reason,
        article: entry.article,
        list: entry.list,
    })
}

fn wire_for_event(event: &SignedEventJson) -> ContentTreeWire {
    tokenize_with_kind(&event.content, &event.tags, RenderMode::Auto, event.kind).to_wire()
}
