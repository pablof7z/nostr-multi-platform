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
use serde::Serialize;

const ANDROID_BUNDLE_PATH: &str = "android/gallery/src/main/assets/content-gallery-bundle.json";
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
fn convert_embed(uri: &str, entry: EmbedEntry) -> WireEmbedEnvelope {
    // Collapsed entries (dangling / unsupported): forward the reason, no projection.
    if entry.collapsed {
        return WireEmbedEnvelope {
            collapsed: true,
            collapse_reason: entry.collapse_reason,
            projection: None,
        };
    }

    // Produce a KernelEvent from the entry's underlying event, then resolve via
    // the same embed_projection resolver the production kernel uses.
    let projection = if let Some(ev) = &entry.event {
        let kernel_event = to_kernel_event(ev);
        let ctx = RenderContext::default();
        Some(resolve_embed_projection(&kernel_event, &ctx))
    } else if entry.profile_name.is_some() || entry.profile_picture.is_some() {
        // Profile-only entries (kind:0 mentions without a full event body).
        // Build a minimal Profile projection from the pre-resolved profile fields.
        use nmp_content::ProfileProjection;
        Some(EmbedKindProjection::Profile(ProfileProjection {
            pubkey: uri_to_pubkey(uri),
            display_name: entry.profile_name,
            picture_url: entry.profile_picture,
            about: None,
            nip05: None,
            lud16: None,
            banner_url: None,
        }))
    } else {
        // No event and no profile data — treat as dangling.
        return WireEmbedEnvelope {
            collapsed: true,
            collapse_reason: Some("dangling".to_string()),
            projection: None,
        };
    };

    WireEmbedEnvelope {
        collapsed: false,
        collapse_reason: None,
        projection,
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

/// Best-effort pubkey extraction from a `nostr:npub…` / `nostr:nprofile…` URI.
/// The fixture URIs are well-formed; if extraction fails we use the URI itself
/// as a sentinel so the profile tile degrades gracefully.
fn uri_to_pubkey(uri: &str) -> String {
    // The embed store keys profiles by their nostr: URI. Strip the prefix and
    // decode the bech32 to get the raw hex pubkey. Rather than pulling in the
    // full nostr / bech32 stack here we extract the hex via the fixture's
    // `cycle_key` convention — but since we don't have it here, we use the URI
    // as an opaque identifier. In the gallery context `pubkey` is only used for
    // the Identicon avatar which accepts any deterministic byte source.
    uri.trim_start_matches("nostr:").to_string()
}

fn wire_for_event(event: &SignedEventJson) -> ContentTreeWire {
    tokenize_with_kind(&event.content, &event.tags, RenderMode::Auto, event.kind).to_wire()
}
