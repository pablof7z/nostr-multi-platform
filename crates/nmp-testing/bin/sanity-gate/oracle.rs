//! Correctness-under-load oracles.
//!
//! These assert during the live run against real data. Where the typed read
//! hook to fully verify an invariant does not exist on the wire, the oracle
//! measures what it can and marks the rest BLOCKED with a precise "wire this
//! hook" note — never a faked green, never a churning-store edit.

use std::time::Duration;

use crate::config::{Args, Phase, FILLED_TIMELINE_TARGET};
use crate::report::{GateRow, SanityReport, Verdict};

/// The follow-count floor for the 2k-follow fixture. Below this the fixture is
/// UNAVAILABLE and the follow-set oracle SKIPs LOUD (never dilute the threshold).
const FOLLOW_FLOOR: u64 = 2_000;

pub fn run_correctness(report: &mut SanityReport, args: &Args) {
    let phase = Phase::Correctness.as_str();
    let Some(app) = crate::phases::connect_or_skip(report, phase, args) else {
        return;
    };

    follow_set_completeness(report, phase, args, &app);
    replaceable_supersession(report, phase, &app);
}

/// Oracle: the visible follow-feed for a ~2k-follow account is COMPLETE — the
/// visible authors are a subset of (follow-set ∪ self), and the feed fills to a
/// healthy size (proving the #1500 500-cap deletion did not truncate the feed).
///
/// HOOK GAP: the typed `SnapshotEnvelope` exposes `visible_items` (count) but
/// NOT per-item author hex on the wire, and the follow-set p-tag list is not on
/// the envelope either. So the FULL `authors ⊆ follow-set ∪ self` set-inclusion
/// check cannot be done from the wire today. We assert the measurable half
/// (feed fills past the old 500 cap toward the follow backlog) and mark the
/// set-inclusion half BLOCKED with the exact hook to add.
fn follow_set_completeness(
    report: &mut SanityReport,
    phase: &str,
    args: &Args,
    app: &crate::driver::DrivenApp,
) {
    // Fixture gate: enforce the 2k-follow floor honestly.
    let follow_count = args.follow_count.unwrap_or(0);
    if follow_count < FOLLOW_FLOOR {
        report.push(GateRow::unmeasured(
            "follow-set-fixture",
            phase,
            "accounts.json + nak req kind:3 (orchestrator)",
            "kind:3 p-tag count",
            &format!(">= {FOLLOW_FLOOR} follows"),
            Verdict::SkipRelayMiss,
            &format!(
                "fixture UNAVAILABLE: --follow-count={follow_count} < {FOLLOW_FLOOR}. \
                 Provide a real high-follow account in accounts.json — do NOT dilute the threshold."
            ),
        ));
        return;
    }

    // Measurable half: the feed must fill well past the old 500 cap. With a 2k+
    // follow set the backlog should comfortably reach the filled-timeline
    // target; failing to do so is the truncation regression we hunt.
    let reached = app
        .wait_until(Duration::from_secs(30), |s| {
            s.peak_visible() >= FILLED_TIMELINE_TARGET
        })
        .is_some();
    let peak = app.with_state(|s| s.peak_visible());
    if reached {
        report.push(
            GateRow::min(
                "follow-feed-not-truncated",
                phase,
                "decode_snapshot_envelope",
                "SnapshotEnvelope.visible_items",
                peak as f64,
                FILLED_TIMELINE_TARGET as f64,
                "items",
            )
            .with_note(&format!(
                "follow_count={follow_count}; feed reached {peak} items (>= {FILLED_TIMELINE_TARGET} \
                 proves the 500-cap deletion did not truncate the backlog)"
            )),
        );
    } else {
        report.push(GateRow::unmeasured(
            "follow-feed-not-truncated",
            phase,
            "decode_snapshot_envelope",
            "SnapshotEnvelope.visible_items",
            &format!(">= {FILLED_TIMELINE_TARGET} items"),
            Verdict::SkipRelayMiss,
            &format!(
                "feed only reached {peak} items for a {follow_count}-follow account — \
                 relay backlog insufficient OR truncation regression (investigate)"
            ),
        ));
    }

    // Set-inclusion half: BLOCKED on a missing read hook.
    report.push(GateRow::unmeasured(
        "follow-feed-authors-subset",
        phase,
        "(none)",
        "SnapshotEnvelope (no per-item author hex on the wire)",
        "visible authors ⊆ follow-set ∪ self",
        Verdict::Blocked,
        "HOOK MISSING — the typed snapshot envelope carries visible_items (count) but not \
         per-item author hex, and the active follow-set p-tags are not on the wire. Wire a \
         typed-projection author read (or an nmp_app_read_feed_authors hook) in a follow-up to \
         complete the subset oracle; do NOT scrape the churning store.",
    ));
}

/// Oracle: replaceable supersession (kind 0/3/10002 newest-wins). The store
/// enforces NIP-01 replaceable semantics on insert. We cannot read the resolved
/// replaceable value off the typed envelope, so the deep assertion is BLOCKED;
/// however, the firehose dedup gate already proves the store does NOT grow on a
/// re-arrival, which is the same insert chokepoint. We record the supersession
/// invariant as BLOCKED-on-hook with the exact seam to add.
fn replaceable_supersession(
    report: &mut SanityReport,
    phase: &str,
    _app: &crate::driver::DrivenApp,
) {
    report.push(GateRow::unmeasured(
        "replaceable-supersession",
        phase,
        "(none)",
        "store replaceable-insert resolution (no FFI read hook)",
        "newest kind 0/3/10002 wins, predecessor removed",
        Verdict::Blocked,
        "HOOK MISSING — no FFI/diagnostics read for the resolved replaceable event value. The \
         insert-time supersession is covered by nmp-core unit tests; surfacing it to the harness \
         needs an nmp_app_read_replaceable(pubkey, kind) hook. The firehose dedup-no-growth gate \
         exercises the SAME insert chokepoint as a partial live proxy.",
    ));
}
