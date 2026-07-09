use std::fs;
use std::path::{Path, PathBuf};

const CORE_ROOT_EXPORTS: &[&str] = &[
    "pub mod actor;",
    "pub mod bunker_hook;",
    "pub mod external_signer_hook;",
    "pub mod capability_socket;",
    "pub mod display;",
    "pub mod kinds;",
    "pub mod browse;",
    "pub mod publish;",
    "pub use transport::dispatch_envelope;",
    "pub mod projection_emission;",
    "pub mod refs;",
    "pub mod slots;",
    "pub mod subs;",
    "pub mod substrate;",
    "pub mod tags;",
    "pub mod time;",
    // doctrine-allow: #3116 — the reusable Trellis-backed keyed-reconciler
    // core (#3115/#3116): `nmp-read-session::demand_set` and (a follow-up
    // migration) `kernel::feed_author_refs` both consume it, so it must sit
    // above `nmp-core`'s own private `kernel` module, not inside it.
    "pub mod trellis_reconciler;",
    "pub mod ui_token;",
    "pub mod util;",
    "pub use app::{ resolve_open_uri, KernelAction, KernelUpdate, KernelViewSpec, OpenUriError, OpenUriRouting, VIEW_ADDRESSABLE, VIEW_PROFILE, VIEW_THREAD, };",
    "pub use bunker_hook::{ install_bunker_hook, new_bunker_hook_slot, BunkerHookFn, BunkerHookRequest, BunkerHookSlot, };",
    "pub use external_signer_hook::{ install_external_signer_hook, new_external_signer_hook_slot, ExternalSignerHookFn, ExternalSignerHookRequest, ExternalSignerHookSlot, };",
    // doctrine-allow: #2868 — `WireSubscriptionDiagnosticSnapshot` is the neutral
    // wire-subscription diagnostic seam the dev-only `nmp-devtools` sidecar reads
    // (devtools -> core edge; core never depends on devtools).
    "pub use kernel::{ read_eligible_relay_urls, AppRelay, AppRelayList, AppRelaySlot, DependentInterestChild, DependentInterestDelta, DependentInterestDeltaCommand, Kernel, ProfileLiveness, WireSubscriptionDiagnosticSnapshot, KERNEL_BUILTIN_PROJECTION_KEYS, };",
    "pub use kernel::pull::{pull_page_over, PullError, PullLimits, PullScope};",
    "pub use kernel::pull_cursor::{InvalidCursorSpec, PullConsumerId, PullCursorHandle};",
    "pub use kernel::pull_cursor::{PullCursorId, PullCursorMode, PullCursorRegistry, PullCursorSpec};",
    "pub use kernel::pull_wake::{decode_pull_wake_batch, PullWakeRow, PULL_WAKE_KEY};",
    "pub use kernel::{record_emitted_feed_authors, EmittedFeedAuthorsSlot};",
    "pub use kernel::{ EventShape, ProfileShape, RefLiveness, RefNamespace, RefResolveMetadata, RefShape, };",
    "pub use kernel::{default_registry, ActionRegistry};",
    "pub use kernel::{ CompositionLedger, CompositionRecord, Disposition, COMPOSITION_REPORT_SCHEMA_VERSION, };",
    "pub use kernel::Clock;",
    "pub use kernel::MonotonicSecondClock;",
    "pub mod relay_score { ... }",
    "pub use kernel::{wallet_access::KernelWalletAccess, AuthSignerFn};",
    "pub use kernel::routing_trace::{ PublishTraceEntry, RoutingTraceProjection, SubscriptionTraceEntry, DEFAULT_ROUTING_TRACE_CAPACITY, };",
    "pub use kernel::routing_trace_dto::{projection_to_json, ROUTING_TRACE_SCHEMA_VERSION};",
    "pub use kernel::{ kernel_ports::{ IdentityPort, InterestPort, KernelPorts, ProtocolDispatchPort, PublishPort, PullCursorPort, ReferencePort, RelayLifecyclePort, UiPort, }, RelayFrame, };",
    "pub use kernel_reducer::CommandApplyOutcome;",
    "pub use kernel_reducer::{ KernelReducer, SignRoundTripCompletion, SignRoundTripOutcome, SignRoundTripRequest, };",
    "pub use relay::canonical_relay_url;",
    "pub use relay::OutboundMessage;",
    "pub use update_envelope::{ decode_snapshot_envelope, decode_snapshot_typed_projections, decode_update_frame, encode_panic, encode_snapshot_frame, panic_message, PanicFrame, ProjectionMergeCache, RelayStatusEntry, SnapshotEnvelope, TypedProjectionData, UpdateEnvelope, UpdateFrameBytes, UpdateFrameDecodeError, WireProjectionState, WireSubscriptionEntry, SNAPSHOT_SCHEMA_VERSION, };",
    "pub mod typed_projections { ... }",
    "pub use actor::typed_projections::{ decode_signer_state, encode_signer_state, SignerStateModel, SIGNER_STATE_FILE_IDENTIFIER, SIGNER_STATE_SCHEMA_ID, SIGNER_STATE_SCHEMA_VERSION, };",
    "pub use signer_state_codec::{ decode_signer_state, encode_signer_state, SignerStateModel, SIGNER_STATE_FILE_IDENTIFIER, SIGNER_STATE_SCHEMA_ID, SIGNER_STATE_SCHEMA_VERSION, };",
    "pub use actor::{CipherContinuation, SignContinuation, SignerSource};",
    "pub use actor::ActorMail;",
    "pub use actor::{CommandSendError, CommandSendStatus, CommandSender};",
    "pub use actor::{LIFECYCLE_PHASE_BACKGROUND, LIFECYCLE_PHASE_FOREGROUND};",
    "pub use actor::{ObservedProjectionId, ObservedProjectionSink};",
    "pub use actor::KindFilter;",
    "pub mod __ffi_internal { ... }",
    "pub mod testing;",
    "pub mod ownership;",
];

const CORE_TYPED_DECODER_FILES: &[&str] = &[
    "accounts_fb.rs",
    "accounts_producer_consts.generated.rs",
    "action_lifecycle_fb.rs",
    "action_lifecycle_producer_consts.generated.rs",
    "action_results_fb.rs",
    "action_results_producer_consts.generated.rs",
    "action_stages_fb.rs",
    "action_stages_producer_consts.generated.rs",
    "active_account_fb.rs",
    "active_account_producer_consts.generated.rs",
    "claimed_events_fb.rs",
    "configured_relays_fb.rs",
    "configured_relays_producer_consts.generated.rs",
    "outbox_summary_fb.rs",
    "outbox_summary_producer_consts.generated.rs",
    "profile_fb.rs",
    "profile_producer_consts.generated.rs",
    "publish_outbox_fb.rs",
    "publish_outbox_producer_consts.generated.rs",
    "publish_queue_fb.rs",
    "publish_queue_producer_consts.generated.rs",
    "relay_diagnostics_fb.rs",
    "relay_diagnostics_producer_consts.generated.rs",
    "relay_role_options_fb.rs",
    "relay_role_options_producer_consts.generated.rs",
    "settings_hub_fb.rs",
    "settings_hub_producer_consts.generated.rs",
    "signed_events_fb.rs",
    "signed_events_producer_consts.generated.rs",
];

#[test]
fn nmp_core_root_exports_match_pre_v1_baseline() {
    let root = super::workspace_root();
    let lib = root.join("crates/nmp-core/src/lib.rs");
    let exports = extract_public_root_exports(&lib);
    assert_eq!(
        exports,
        CORE_ROOT_EXPORTS,
        "nmp-core crate-root public surface changed; update this baseline only with a doctrine-allow rationale"
    );
}

#[test]
fn nmp_core_flatbuffer_decoder_files_match_baseline() {
    let root = super::workspace_root();
    let dir = root.join("crates/nmp-core/src/kernel/typed_projections");
    let mut files = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with("_fb.rs") || name.ends_with("_producer_consts.generated.rs"))
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(
        files,
        CORE_TYPED_DECODER_FILES,
        "new pure FlatBuffers decoders in nmp-core require an extraction issue or explicit baseline update"
    );
}

#[test]
fn protocol_crates_do_not_import_kernel_or_actor_runtime() {
    let root = super::workspace_root();
    let mut findings = Vec::new();
    for crate_dir in protocol_crate_roots(&root) {
        collect_runtime_imports(&crate_dir.join("src"), &mut findings);
    }
    assert!(
        findings.is_empty(),
        "protocol crates must not import kernel internals or actor runtime types:\n{}",
        findings.join("\n")
    );
}

fn protocol_crate_roots(root: &Path) -> Vec<PathBuf> {
    let crates = root.join("crates");
    let mut out = Vec::new();
    for entry in fs::read_dir(crates).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("nmp-nip") || name == "nmp-content" || name.starts_with("nmp-feed") {
            out.push(path);
        }
    }
    out
}

fn collect_runtime_imports(dir: &Path, findings: &mut Vec<String>) {
    if !dir.exists() {
        return;
    }
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_runtime_imports(&path, findings);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            scan_runtime_import_file(&path, findings);
        }
    }
}

fn scan_runtime_import_file(path: &Path, findings: &mut Vec<String>) {
    let raw = fs::read_to_string(path).unwrap();
    for (index, line) in raw.lines().enumerate() {
        let line = line.split("//").next().unwrap_or("").trim();
        if line.contains("nmp_core::kernel::")
            || line.contains("ActorRuntime")
            || line.contains("ActorChannels")
            || line.contains("spawn_test_actor")
            || line.contains("run_actor_with_observers")
        {
            findings.push(format!("{}:{}: {}", path.display(), index + 1, line));
        }
    }
}

fn extract_public_root_exports(path: &Path) -> Vec<String> {
    let raw = fs::read_to_string(path).unwrap();
    let lines = raw.lines().collect::<Vec<_>>();
    let mut out = Vec::new();
    let mut depth = 0isize;
    let mut index = 0usize;
    while index < lines.len() {
        let stripped = strip_comment(lines[index]);
        if depth == 0 && (stripped.starts_with("pub mod ") || stripped.starts_with("pub use ")) {
            if stripped.starts_with("pub use ") && !stripped.contains(';') {
                let mut statement = stripped;
                update_depth_until(lines[index], None, &mut depth);
                while index + 1 < lines.len() && !statement.contains(';') {
                    index += 1;
                    let next = strip_comment(lines[index]);
                    if !next.is_empty() {
                        statement.push(' ');
                        statement.push_str(&next);
                    }
                    update_depth_until(lines[index], lines[index].find(';'), &mut depth);
                }
                out.push(normalize_statement(
                    &statement[..statement.find(';').unwrap() + 1],
                ));
                index += 1;
                continue;
            } else if stripped.starts_with("pub mod ") && stripped.contains('{') {
                let name = stripped
                    .trim_start_matches("pub mod ")
                    .split_whitespace()
                    .next()
                    .unwrap();
                out.push(format!("pub mod {name} {{ ... }}"));
            } else {
                let end = stripped
                    .find(';')
                    .map(|pos| pos + 1)
                    .unwrap_or(stripped.len());
                out.push(normalize_statement(&stripped[..end]));
            }
        }
        update_depth_until(lines[index], None, &mut depth);
        index += 1;
    }
    out
}

fn strip_comment(line: &str) -> String {
    line.split("//").next().unwrap_or("").trim().to_string()
}

fn normalize_statement(statement: &str) -> String {
    statement.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn update_depth_until(line: &str, stop: Option<usize>, depth: &mut isize) {
    let end = stop.map(|pos| pos + 1).unwrap_or(line.len());
    for ch in line[..end].chars() {
        match ch {
            '{' => *depth += 1,
            '}' => *depth -= 1,
            _ => {}
        }
    }
}
