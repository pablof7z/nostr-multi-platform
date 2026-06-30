use std::{
    collections::HashSet,
    sync::{mpsc::Receiver, Arc},
    time::{Duration, Instant},
};

use nmp_content::EventRefResolver;
use nmp_gallery_tui::{
    embed_host::EmbedHostState,
    live::{GalleryTypedSnapshot, LiveKernelSink},
};
use nmp_nostr_id::{decode_naddr, decode_nevent, decode_note};

use crate::smoke_display::{print_resolved, projection_label};

struct SmokeTarget {
    label: &'static str,
    uri: String,
    /// Row key the kernel uses for `refs.event[primary_id]`.
    /// hex64 event id for nevent/note; "kind:author:d_tag" for naddr.
    primary_id: String,
}

/// Headless verification of the renderer-triggered resolve path. Mirrors what
/// the TUI does at render time but without ratatui.
pub(crate) fn run(
    sink: &Arc<LiveKernelSink>,
    host: &mut EmbedHostState,
    snapshot_rx: Receiver<Vec<u8>>,
    timeout: Duration,
) -> i32 {
    let Some(targets) = smoke_targets() else {
        return 1;
    };
    let consumer_id = "nmp-gallery-tui.smoke";

    println!("== nmp-gallery-tui --smoke ==");
    println!("kernel up, relays seeded; validating renderer-triggered event-ref resolves.");
    println!();

    println!(
        "Target {} embed URI(s); waiting for relay connection then resolving:",
        targets.len()
    );
    for t in &targets {
        println!("  target: {} -> {}", t.label, t.uri);
        println!("    primary_id expected in refs.event: {}", t.primary_id);
    }
    println!();

    let result = drain_until_resolved(sink, host, snapshot_rx, timeout, &targets, consumer_id);
    print_summary(result, host, &targets)
}

fn smoke_targets() -> Option<Vec<SmokeTarget>> {
    let mut targets = Vec::new();
    for (label, uri) in [
        (
            "embed_article (kind:30023 naddr)",
            nmp_gallery_tui::data::article_naddr().to_string(),
        ),
        (
            "embed_note (kind:1 nevent)",
            nmp_gallery_tui::data::note_nevent().to_string(),
        ),
    ] {
        match primary_id_for(&uri) {
            Some(primary_id) => targets.push(SmokeTarget {
                label,
                uri,
                primary_id,
            }),
            None => {
                eprintln!("smoke: could not decode URI for {label}: {uri}");
                return None;
            }
        }
    }
    Some(targets)
}

fn primary_id_for(uri: &str) -> Option<String> {
    let stripped = uri.strip_prefix("nostr:").unwrap_or(uri);
    if let Ok(naddr) = decode_naddr(stripped) {
        return Some(format!(
            "{}:{}:{}",
            naddr.kind, naddr.pubkey, naddr.identifier
        ));
    }
    if let Ok(nevent) = decode_nevent(stripped) {
        return Some(nevent.event_id);
    }
    if let Ok(note) = decode_note(stripped) {
        return Some(note);
    }
    None
}

struct SmokeResult {
    resolved_ids: HashSet<String>,
    resolves_issued: bool,
    snapshot_ticks: u32,
    elapsed: Duration,
    disconnected: bool,
}

fn drain_until_resolved(
    sink: &Arc<LiveKernelSink>,
    host: &mut EmbedHostState,
    snapshot_rx: Receiver<Vec<u8>>,
    timeout: Duration,
    targets: &[SmokeTarget],
    consumer_id: &str,
) -> SmokeResult {
    let started = Instant::now();
    let mut result = SmokeResult {
        resolved_ids: HashSet::new(),
        resolves_issued: false,
        snapshot_ticks: 0,
        elapsed: Duration::ZERO,
        disconnected: false,
    };
    let mut ref_profiles = nmp_core::refs::RefProfileStore::new();
    let mut ref_events = nmp_core::refs::RefEventStore::new();

    while started.elapsed() < timeout && result.resolved_ids.len() < targets.len() {
        let remaining = timeout - started.elapsed();
        match snapshot_rx.recv_timeout(remaining) {
            Ok(frame_bytes) => {
                let snap = GalleryTypedSnapshot::from_frame_bytes(
                    &frame_bytes,
                    &mut ref_profiles,
                    &mut ref_events,
                );
                result.snapshot_ticks += 1;
                host.update_from_typed(&snap);

                if !result.resolves_issued && snap.any_relay_connected() {
                    println!(
                        "  + relay connected - event-ref resolves firing on tick #{}",
                        result.snapshot_ticks
                    );
                    for t in targets {
                        println!("    resolve: {}", t.uri);
                        sink.resolve_event_ref(&t.uri, consumer_id);
                    }
                    result.resolves_issued = true;
                }

                record_resolved_targets(host, targets, &mut result.resolved_ids, started);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                eprintln!("snapshot channel disconnected before targets resolved");
                result.disconnected = true;
                break;
            }
        }
    }

    result.elapsed = started.elapsed();
    result
}

fn record_resolved_targets(
    host: &EmbedHostState,
    targets: &[SmokeTarget],
    resolved_ids: &mut HashSet<String>,
    started: Instant,
) {
    for t in targets {
        if resolved_ids.contains(&t.primary_id) {
            continue;
        }
        if let Some(envelope) = host.current_envelopes().get(&t.primary_id) {
            println!(
                "+ resolved at {:.2}s: {}",
                started.elapsed().as_secs_f32(),
                t.label
            );
            print_resolved(t.label, envelope);
            resolved_ids.insert(t.primary_id.clone());
        }
    }
}

fn print_summary(result: SmokeResult, host: &EmbedHostState, targets: &[SmokeTarget]) -> i32 {
    println!();
    println!("Summary:");
    println!("  snapshot ticks observed: {}", result.snapshot_ticks);
    println!(
        "  resolves issued:         {}",
        if result.resolves_issued { "yes" } else { "no" }
    );
    println!(
        "  resolved targets:        {}/{}",
        result.resolved_ids.len(),
        targets.len()
    );
    println!();

    if result.disconnected {
        return 1;
    }

    let unresolved: Vec<&SmokeTarget> = targets
        .iter()
        .filter(|t| !result.resolved_ids.contains(&t.primary_id))
        .collect();
    if unresolved.is_empty() {
        println!(
            "ALL {} embed targets resolved in {:.2}s",
            targets.len(),
            result.elapsed.as_secs_f32()
        );
        return 0;
    }

    println!(
        "{}/{} targets unresolved after {:.2}s:",
        unresolved.len(),
        targets.len(),
        result.elapsed.as_secs_f32()
    );
    for t in &unresolved {
        println!(
            "  unresolved: {} -> {} (expected primary_id: {})",
            t.label, t.uri, t.primary_id
        );
    }
    println!();
    println!("  Most likely cause: the target event isn't on the seeded relays.");
    print!("  The seeded relays are:");
    for r in &nmp_app_gallery::showcase::references().relays {
        print!(" {} ({})", r.url, r.role);
    }
    println!(". Architecture is validated by the resolved targets above.");
    println!(
        "Host envelope map ({} entries):",
        host.current_envelopes().len()
    );
    for (k, env) in host.current_envelopes() {
        println!("  - {k} -> {}", projection_label(&env.projection));
    }
    1
}
