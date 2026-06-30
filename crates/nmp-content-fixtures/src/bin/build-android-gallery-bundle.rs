//! Build the Android content-gallery bundle in the kind-registry wire shape.
//!
//! Each embed entry now carries an `EmbedKindProjection` envelope (the same
//! typed shape the production NEMB sidecar delivers) so the Android `:gallery`
//! module can dispatch through `GalleryKindRegistry` exactly like the app.
//!
//! Usage: `cargo run -p nmp-content-fixtures --bin build-android-gallery-bundle`
//! from the workspace root.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use nmp_content::{
    resolve_embed_projection, tokenize_with_kind, ContentTreeWire, EmbedKindProjection,
    RenderContext, RenderMode,
};
use nmp_content_fixtures::{
    build_bundle,
    dto::{EmbedEntry, ScenarioDto, SignedEventJson},
};
use nmp_core::substrate::KernelEvent;
use nmp_nostr_id::{parse_nostr_uri, NostrUri};
use serde::Serialize;

const ANDROID_BUNDLE_PATH: &str =
    "apps/nmp-gallery/android/app/src/main/assets/content-gallery-bundle.json";
/// Bump to 3 to signal the new embed shape; decoders must upgrade.
const ANDROID_BUNDLE_VERSION: u32 = 3;

// ─────────────────────────────────────────────────────────────────────────────
// Wire shapes (gallery-only; NOT the production NEMB FlatBuffers shape).
// ─────────────────────────────────────────────────────────────────────────────

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
    embeds: BTreeMap<String, WireEmbedEnvelope>,
}

/// Gallery-bundle embed envelope: collapsed flag + kind-registry projection.
/// Mirrors the production `EmbeddedEventEnvelope` shape without the FlatBuffers
/// overhead — the gallery decodes JSON, not a NEMB binary sidecar.
#[derive(Serialize)]
struct WireEmbedEnvelope {
    /// Whether the embed is collapsed (dangling / depth / cycle / unsupported).
    collapsed: bool,
    /// Machine-readable collapse reason, or `null` when not collapsed.
    #[serde(skip_serializing_if = "Option::is_none")]
    collapse_reason: Option<String>,
    /// Kind-dispatched projection, present when `collapsed == false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    projection: Option<EmbedKindProjection>,
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
            .map_err(|err| format!("create {} failed: {}", parent.display(), err))?;
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
            let envelope = convert_embed(&uri, entry);
            (uri, envelope)
        })
        .collect::<BTreeMap<_, _>>();
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

/// Convert a legacy `EmbedEntry` (from the fixture store) into the new
/// kind-registry `WireEmbedEnvelope` shape.
///
/// Every resolvable entry — whether it carries a full event body or is a bare
/// kind:0 profile mention — is routed through the SAME `resolve_embed_projection`
/// resolver the production kernel uses. This guarantees the bundle's projection
/// shape (raw lowercase-hex pubkeys per ADR-0032, the same display-name
/// precedence, etc.) is byte-identical to production. There is no hand-built
/// `EmbedKindProjection` here.
fn convert_embed(uri: &str, entry: EmbedEntry) -> WireEmbedEnvelope {
    // Collapsed entries (dangling / unsupported): forward the reason, no projection.
    if entry.collapsed {
        return WireEmbedEnvelope {
            collapsed: true,
            collapse_reason: entry.collapse_reason,
            projection: None,
        };
    }

    // Build the KernelEvent to resolve. Event-backed entries use the underlying
    // event verbatim; profile-only entries (kind:0 mentions with no full event
    // body in the relay-free store) synthesize the equivalent kind:0 event so
    // the SAME resolver parses the profile metadata — including emitting the
    // raw-hex pubkey, not the bech32 URI.
    let kernel_event = if let Some(ev) = &entry.event {
        to_kernel_event(ev)
    } else if entry.profile_name.is_some() || entry.profile_picture.is_some() {
        match profile_kernel_event(uri, &entry) {
            Some(ev) => ev,
            None => {
                // URI couldn't be decoded to a raw-hex pubkey — degrade to a
                // dangling stub rather than emit a non-hex pubkey on the wire.
                return WireEmbedEnvelope {
                    collapsed: true,
                    collapse_reason: Some("dangling".to_string()),
                    projection: None,
                };
            }
        }
    } else {
        // No event and no profile data — treat as dangling.
        return WireEmbedEnvelope {
            collapsed: true,
            collapse_reason: Some("dangling".to_string()),
            projection: None,
        };
    };

    let ctx = RenderContext::default();
    WireEmbedEnvelope {
        collapsed: false,
        collapse_reason: None,
        projection: Some(resolve_embed_projection(&kernel_event, &ctx)),
    }
}

/// Produce a minimal `KernelEvent` from the fixture's `SignedEventJson`.
fn to_kernel_event(ev: &SignedEventJson) -> KernelEvent {
    KernelEvent {
        id: ev.id.clone(),
        author: ev.pubkey.clone(),
        kind: ev.kind,
        created_at: ev.created_at,
        tags: ev.tags.clone(),
        content: ev.content.clone(),
        relay_provenance: vec![],
    }
}

/// Synthesize the kind:0 `KernelEvent` for a profile-only embed so it can flow
/// through `resolve_embed_projection` exactly like an event-backed embed.
///
/// The `author` field is the RAW lowercase-hex pubkey decoded from the
/// `nostr:npub…` / `nostr:nprofile…` URI (ADR-0032: the wire carries raw hex;
/// the shell encodes bech32 for display). The `content` is the NIP-01 kind:0
/// metadata JSON reconstructed from the store's resolved name/picture — the
/// resolver's `parse_profile_metadata` reads it back into a `ProfileProjection`.
/// Returns `None` when the URI is not a decodable profile reference.
fn profile_kernel_event(uri: &str, entry: &EmbedEntry) -> Option<KernelEvent> {
    let pubkey_hex = profile_pubkey_hex(uri)?;
    let metadata = profile_metadata_json(
        entry.profile_name.as_deref(),
        entry.profile_picture.as_deref(),
    );
    Some(KernelEvent {
        id: String::new(),
        author: pubkey_hex,
        kind: 0,
        created_at: 0,
        tags: vec![],
        content: metadata,
        relay_provenance: vec![],
    })
}

/// Decode a `nostr:npub…` / `nostr:nprofile…` URI to its raw lowercase-hex
/// pubkey. Returns `None` for any non-profile or undecodable URI.
fn profile_pubkey_hex(uri: &str) -> Option<String> {
    match parse_nostr_uri(uri).ok()? {
        NostrUri::Profile { pubkey, .. } => Some(pubkey),
        _ => None,
    }
}

/// Reconstruct the NIP-01 kind:0 metadata JSON from the store's resolved
/// profile fields, so the SAME resolver path parses it. Only the fields the
/// fixture store carries (`name`, `picture`) are populated.
fn profile_metadata_json(name: Option<&str>, picture: Option<&str>) -> String {
    let mut map = serde_json::Map::new();
    if let Some(name) = name {
        map.insert(
            "name".to_string(),
            serde_json::Value::String(name.to_string()),
        );
    }
    if let Some(picture) = picture {
        map.insert(
            "picture".to_string(),
            serde_json::Value::String(picture.to_string()),
        );
    }
    serde_json::Value::Object(map).to_string()
}

fn wire_for_event(event: &SignedEventJson) -> ContentTreeWire {
    tokenize_with_kind(&event.content, &event.tags, RenderMode::Auto, event.kind).to_wire()
}
