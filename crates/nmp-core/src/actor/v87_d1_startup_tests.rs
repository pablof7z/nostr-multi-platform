//! V-87 D1 offline-first startup ordering tests.
//!
//! Two properties must hold for D1 / offline-first.md §3:
//!
//! 1. **Pre-command frame** (`#601`) — the actor emits at least one snapshot
//!    BEFORE the host sends any command.  A host that waits for the first frame
//!    before calling `Start` must not deadlock.
//!
//! 2. **Zero-relay startup** (`#602`) — a `Start` with zero relays connected
//!    must not block indefinitely and must emit at least one running snapshot.
//!    `maybe_send_startup` must not be gated on `all_relays_connected`.
//!
//! 3. **Rev monotonicity** (`#601-rev`) — the real kernel's first
//!    `running=true` frame MUST carry a `rev` strictly greater than the
//!    pre-flight frame's `rev`, so the iOS host's
//!    `guard update.rev > rev` (KernelModel.swift:643) never silently drops
//!    it.  Without the `resume_rev_after_preflight` fix both frames carry
//!    `rev=1` and the host drops the `running=true` frame, leaving the UI
//!    stuck on the `running=false` pre-flight state indefinitely.
//!
//! 4. **Seeded-store offline render** (`#628`) — an actor whose local store is
//!    seeded with a known event BEFORE `Start` (with zero relays) must render
//!    that event from the local store alone. The running snapshot's typed
//!    `claimed_events` sidecar must contain the seeded event within the same
//!    500 ms budget, with no relay connectivity at all
//!    (offline-first.md §7: "the first rendered frame is produced from
//!    local-store content alone").
//!
//! 5. **Emit-before-dial ordering** (`#600` / `#628`) — the `Start` arm
//!    (`dispatch.rs` ~460-461) MUST call `emit_now` BEFORE `spawn_missing_relays`
//!    so the first snapshot reaches the shell before any relay TCP connection is
//!    dialed. The test drives the real `Start` dispatch with ONE configured
//!    relay and asserts the first `running=true` frame does NOT yet mark that
//!    relay `"connecting"` (the dial, via `kernel.relay_connecting_url`, has not
//!    run when the frame is encoded). A later frame DOES show `"connecting"`,
//!    proving the dial happened — so the ordering assertion is non-vacuous.

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::actor::{run_actor, ActorCommand, ActorMail, CommandSender};
    use crate::relay::DEFAULT_VISIBLE_LIMIT;
    use crate::update_envelope::{decode_snapshot_envelope, SnapshotEnvelope};

    // ─── helper ─────────────────────────────────────────────────────────────

    fn spawn_actor() -> (
        CommandSender,
        mpsc::Receiver<crate::update_envelope::UpdateFrameBytes>,
    ) {
        let (inbox_tx, cmd_rx) = mpsc::channel::<ActorMail>();
        let cmd_tx = CommandSender::new(inbox_tx);
        let (upd_tx, upd_rx) = mpsc::channel::<crate::update_envelope::UpdateFrameBytes>();
        let actor_self_tx = cmd_tx.clone();
        thread::spawn(move || run_actor(cmd_rx, actor_self_tx, upd_tx));
        (cmd_tx, upd_rx)
    }

    /// PR-B (#991/#979): drain frames from `upd_rx` and decode them via the
    /// typed `SnapshotEnvelope` path (payload is no longer emitted on the wire).
    /// Returns the typed `SnapshotEnvelope` for every snapshot frame received.
    fn drain_snapshots(
        upd_rx: &mpsc::Receiver<crate::update_envelope::UpdateFrameBytes>,
        timeout: Duration,
    ) -> Vec<SnapshotEnvelope> {
        let deadline = std::time::Instant::now() + timeout;
        let mut snapshots = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match upd_rx.recv_timeout(remaining) {
                Ok(frame) => {
                    if let Ok(env) = decode_snapshot_envelope(&frame) {
                        snapshots.push(env);
                    }
                    // Panic frames or malformed bytes are ignored (no payload on
                    // the channel in a healthy test run; a panic would surface as
                    // a decode error on the typed path).
                }
                Err(_) => break,
            }
        }
        snapshots
    }

    // ─── V-87 #601: pre-command frame ────────────────────────────────────────

    /// The actor MUST emit a snapshot before the host sends any command.
    ///
    /// Offline-first.md §3: "the first snapshot is unconditional … even if the
    /// working set is empty." A host that waits for the first frame before
    /// sending `Start` must not deadlock.
    ///
    /// Assertion: spawn the actor, do NOT send Start, and assert that at least
    /// one snapshot arrives on the update channel within a 500 ms window.
    #[test]
    fn v87_601_first_snapshot_arrives_before_start_command() {
        let (_cmd_tx, upd_rx) = spawn_actor();

        // Do NOT send Start or any other command.
        // The actor MUST emit a pre-flight frame independently.
        let snapshots = drain_snapshots(&upd_rx, Duration::from_millis(500));

        assert!(
            !snapshots.is_empty(),
            "V-87 #601: actor must emit at least one snapshot before \
             the host sends any command; offline-first.md §3 requires the \
             first snapshot to be unconditional (got 0 frames in 500 ms)"
        );
        // The pre-flight frame is emitted with `running=false`; the typed
        // envelope's `running` field should be false.
        let first = &snapshots[0];
        assert!(
            !first.running,
            "V-87 #601: pre-flight snapshot must carry running=false, \
             got running=true"
        );
    }

    // ─── V-87 #602: zero-relay startup does not hang ─────────────────────────

    /// A `Start` with zero relay connections must complete and emit a running
    /// snapshot within a bounded budget.
    ///
    /// Offline-first.md §3: "startup MUST NOT wait on a subscription response,
    /// an EOSE, or any relay handshake before emitting its first snapshot."
    ///
    /// Previously `maybe_send_startup` was gated on `all_relays_connected`.
    /// With the fix, `startup_requests()` (bootstrap interest registration) fires
    /// unconditionally when `running=true`, so the planner and snapshot emit path
    /// are not blocked by absent relay connections.
    #[test]
    fn v87_602_start_with_zero_relays_emits_running_snapshot() {
        let (cmd_tx, upd_rx) = spawn_actor();

        // Wait for the pre-flight frame so the actor has had time to build
        // the real kernel.
        let _ = drain_snapshots(&upd_rx, Duration::from_millis(200));

        // Send Start — zero relays are configured (no `AddRelay` beforehand).
        cmd_tx
            .send(ActorCommand::Start {
                visible_limit: DEFAULT_VISIBLE_LIMIT,
                emit_hz: 30,
                initial_relays: Vec::new(),
            })
            .expect("send Start");

        // The actor must emit a snapshot with `running=true` within 500 ms
        // without needing any relay to connect first.
        let snapshots = drain_snapshots(&upd_rx, Duration::from_millis(500));

        let running_snapshot = snapshots.iter().find(|s| s.running);

        assert!(
            running_snapshot.is_some(),
            "V-87 #602: Start with zero relays must produce a running=true \
             snapshot within 500 ms; maybe_send_startup must not be gated on \
             all_relays_connected (got {} total snapshots, none with running=true)",
            snapshots.len()
        );

        // Graceful shutdown.
        let _ = cmd_tx.send(ActorCommand::Shutdown);
    }

    // ─── V-87 combined: no deadlock on snapshot-first host ───────────────────

    /// A host that waits for a snapshot before sending `Start` must not
    /// deadlock — and must receive a running snapshot after sending `Start`.
    ///
    /// **Rev-guard simulation (#601-rev)**: this test faithfully simulates the
    /// shipping iOS host's `guard update.rev > rev` guard
    /// (KernelModel.swift:643).  Frames are only "accepted" if their `rev` is
    /// strictly greater than the last-accepted rev, exactly as the host does.
    ///
    /// Without the `resume_rev_after_preflight` fix:
    /// - Pre-flight frame: `rev=1` → accepted (host had `rev=0`).
    /// - Start frame:      `rev=1` → REJECTED (`1 > 1` is false) → test fails.
    /// - Subsequent idle ticks: `changed_since_emit=false` → no further frames
    ///   → the host stays stuck on the `running=false` state indefinitely.
    ///
    /// With the fix:
    /// - Pre-flight frame: `rev=1` → accepted.
    /// - Start frame:      `rev=2` → accepted (`2 > 1`) → `running=true` → passes.
    #[test]
    fn v87_snapshot_first_host_no_deadlock() {
        let (cmd_tx, upd_rx) = spawn_actor();

        // ── Step 1: receive the unconditional pre-flight frame ───────────────
        let pre_snapshots = drain_snapshots(&upd_rx, Duration::from_millis(500));
        assert!(
            !pre_snapshots.is_empty(),
            "V-87: no pre-flight snapshot arrived within 500 ms — would deadlock \
             a snapshot-first host"
        );

        // Extract the pre-flight frame's rev.  The `rev` field MUST be > 0;
        // if it reads as 0 the host guard is a no-op and this test would pass vacuously.
        let preflight_rev = pre_snapshots[0].rev;
        assert!(
            preflight_rev > 0,
            "V-87: pre-flight rev must be ≥ 1 (got {preflight_rev})"
        );

        // ── Step 2: host sends Start after observing the pre-flight frame ────
        cmd_tx
            .send(ActorCommand::Start {
                visible_limit: DEFAULT_VISIBLE_LIMIT,
                emit_hz: 30,
                initial_relays: Vec::new(),
            })
            .expect("send Start after pre-flight");

        // ── Step 3: simulate the iOS host's `guard update.rev > rev` guard ───
        //
        // Collect post-Start frames and apply the same monotonicity filter the
        // shipping iOS host applies (KernelModel.swift:643).  Only frames with
        // `rev > last_accepted_rev` are "accepted"; the pre-flight frame already
        // moved `last_accepted_rev` to `preflight_rev`.
        let post_snapshots = drain_snapshots(&upd_rx, Duration::from_millis(500));

        // The FIRST post-Start frame MUST have rev strictly greater than
        // `preflight_rev`.  This is the key invariant: the Start dispatch's
        // `emit_now` produces the very first `running=true` frame; if that frame
        // has the same rev as the pre-flight frame the host drops it silently.
        // A host in an offline scenario with no subsequent relay events would
        // receive no further frames (changed_since_emit=false, no relay activity
        // to flip it back true), leaving the UI stuck on running=false forever.
        //
        // Without the fix: pre-flight=rev 1, Start frame=rev 1 → guard drops it.
        // With    the fix: pre-flight=rev 1, Start frame=rev 2 → guard accepts it.
        let first_post_start = post_snapshots
            .first()
            .expect("V-87 #601-rev: no post-Start frames received at all within 500 ms");
        let first_post_start_rev = first_post_start.rev;

        assert!(
            first_post_start_rev > preflight_rev,
            "V-87 #601-rev: Start frame rev={first_post_start_rev} is NOT strictly \
             greater than pre-flight rev={preflight_rev}. \
             The iOS host's `guard update.rev > rev` (KernelModel.swift:643) would \
             silently drop this frame. In an offline scenario with no relay activity, \
             changed_since_emit stays false after the dropped Start emit and no \
             further frames are sent — the host is stuck on running=false indefinitely. \
             Fix: call kernel.resume_rev_after_preflight(preflight_rev) before the \
             dispatch loop so the real kernel's first make_update produces \
             rev = preflight_rev + 1."
        );

        // Belt-and-suspenders: also verify the first accepted frame carries running=true.
        assert!(
            first_post_start.running,
            "V-87 #601-rev: first post-Start frame has rev={first_post_start_rev} > \
             preflight_rev={preflight_rev} (guard passes) but running is not true"
        );

        let _ = cmd_tx.send(ActorCommand::Shutdown);
    }

    // ─── #628: seeded-store offline render ───────────────────────────────────

    /// offline-first.md §7 mandate: "Every viewer-class app MUST have a smoke
    /// test that boots the kernel with zero relay connectivity and verifies
    /// that the first rendered frame is produced from local-store content
    /// alone."
    ///
    /// This test seeds a known event into the actor's local store
    /// (`IngestPreVerifiedEvents`) and claims it (`ClaimEvent`) BEFORE the
    /// `running=true` snapshot is observed, with `initial_relays` EMPTY — zero
    /// connectivity. Within the same 500 ms budget the other D1 tests use, the
    /// actor must emit a `running=true` snapshot whose typed `claimed_events`
    /// sidecar carries the seeded event. No relay ever connects: the rendered
    /// frame is produced from local-store content alone.
    ///
    /// Decode pattern reused from `nmp-testing` C13
    /// (`framework_magic_contract/c5_c8_c13.rs`): `decode_snapshot_typed_projections`
    /// → find the `claimed_events` sidecar → `decode_claimed_events` → look up
    /// the seeded event id.
    #[test]
    fn v628_seeded_store_renders_offline_with_zero_relays() {
        use crate::typed_projections::{decode_claimed_events, CLAIMED_EVENTS_SCHEMA_ID};
        use crate::nip19::encode_note;
        use crate::store::{RawEvent, VerifiedEvent};
        use crate::update_envelope::decode_snapshot_typed_projections;

        let (cmd_tx, upd_rx) = spawn_actor();

        // Drain the unconditional pre-flight frame so the real kernel exists.
        let _ = drain_snapshots(&upd_rx, Duration::from_millis(200));

        // Seed a known event into the local store. The diag-firehose-stress
        // ingest path pushes the event directly into `self.events` regardless
        // of `timeline_authors`, so it is visible to `claimed_events`.
        let author_pk = "628a0628a0628a0628a0628a0628a0628a0628a0628a0628a0628a0628a0628a";
        let event_id = "628e0000628e0000628e0000628e0000628e0000628e0000628e0000628e0000";
        let raw = RawEvent {
            id: event_id.to_string(),
            pubkey: author_pk.to_string(),
            created_at: 1_000,
            kind: 1,
            tags: vec![],
            content: "offline-first seeded note".to_string(),
            sig: "a".repeat(128),
        };
        let verified = VerifiedEvent::from_raw_unchecked(raw);
        cmd_tx
            .send(ActorCommand::IngestPreVerifiedEvents(vec![verified]))
            .expect("seed store");

        // Start with ZERO relays — no connectivity at all.
        cmd_tx
            .send(ActorCommand::Start {
                visible_limit: 100,
                emit_hz: 30,
                initial_relays: Vec::new(),
            })
            .expect("send Start with zero relays");

        // Claim the seeded event so it surfaces in the `claimed_events` typed
        // sidecar (D5: claimed_events carries the entry only after a ClaimEvent
        // dispatch). The store already holds the event, so this resolves from
        // local content — no relay fetch is needed or possible (zero relays).
        let note_uri = format!("nostr:{}", encode_note(event_id).expect("valid note uri"));
        cmd_tx
            .send(ActorCommand::ClaimEvent {
                uri: note_uri,
                consumer_id: "v628-test".to_string(),
                force: false,
            })
            .expect("claim seeded event");

        // Within the existing 500 ms D1 budget, a running=true snapshot whose
        // typed claimed_events sidecar contains the seeded event must arrive —
        // produced from local store alone, with no relay connected.
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        let mut found = false;
        while std::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            match upd_rx.recv_timeout(remaining) {
                Ok(frame) => {
                    // Only consider running=true snapshots (the rendered frame).
                    let env = match decode_snapshot_envelope(&frame) {
                        Ok(env) => env,
                        Err(_) => continue,
                    };
                    if !env.running {
                        continue;
                    }
                    let projections = match decode_snapshot_typed_projections(&frame) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    let has_seeded = projections
                        .iter()
                        .find(|p| p.schema_id == CLAIMED_EVENTS_SCHEMA_ID)
                        .and_then(|p| decode_claimed_events(&p.payload).ok())
                        .map(|model| model.entries.iter().any(|(k, _)| k == event_id))
                        .unwrap_or(false);
                    if has_seeded {
                        found = true;
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        assert!(
            found,
            "#628: a running=true snapshot whose typed claimed_events sidecar \
             contains the seeded event {event_id} must arrive within 500 ms with \
             ZERO relays connected (offline-first.md §7: the first rendered frame \
             is produced from local-store content alone)"
        );

        let _ = cmd_tx.send(ActorCommand::Shutdown);
    }

    // ─── #600: emit-before-dial ordering guard ───────────────────────────────

    /// Regression guard for issue #600: the `Start` arm (`dispatch.rs`
    /// ~460-461) MUST call `emit_now` BEFORE `spawn_missing_relays`, so the
    /// first snapshot reaches the shell before any relay TCP connection is
    /// dialed (D1 / offline-first.md §7).
    ///
    /// This drives the REAL `Start` dispatch through `run_actor` (not a hand-
    /// rolled helper sequence) so it genuinely guards the ordering inside
    /// `dispatch_command`. ONE relay is configured via `initial_relays`, so
    /// `spawn_missing_relays` resolves a bootstrap URL and dials it — which
    /// calls `kernel.relay_connecting_url`, flipping that URL's Tier-3
    /// `relay_statuses` row to `"connecting"`.
    ///
    /// The observable that distinguishes the two orderings:
    ///
    /// - **Correct (emit_now BEFORE spawn_missing_relays):** the very first
    ///   `running=true` frame is encoded *before* the dial, so the configured
    ///   relay's row is NOT yet `"connecting"` in that frame.
    /// - **Reordered (#600 bug — spawn before emit):** the dial has already run
    ///   when the first frame is encoded, so the row reads `"connecting"` — the
    ///   shell's first rendered frame would reflect relay I/O, violating D1.
    ///
    /// We capture the FIRST `running=true` frame and assert the configured
    /// relay is present but not `"connecting"`. Non-vacuous guard: a later frame
    /// (after the dial) MUST show the relay as `"connecting"`, proving the dial
    /// really happened and the ordering assertion was meaningful.
    #[test]
    fn v600_first_running_frame_precedes_relay_dial() {
        // Use a syntactically-valid wss URL that will never connect (TEST-NET-1
        // documentation host, RFC 5737). The dial is non-blocking; we only care
        // that `spawn_missing_relays` marks it `"connecting"` in the kernel.
        let configured_url = "wss://relay.v600.test/";

        let (cmd_tx, upd_rx) = spawn_actor();

        // Drain the unconditional pre-flight frame.
        let _ = drain_snapshots(&upd_rx, Duration::from_millis(200));

        // Start with exactly ONE configured relay (role "both" → Content
        // bootstrap lane). This is the only relay `spawn_missing_relays` dials.
        cmd_tx
            .send(ActorCommand::Start {
                visible_limit: DEFAULT_VISIBLE_LIMIT,
                emit_hz: 30,
                initial_relays: vec![(configured_url.to_string(), "both".to_string())],
            })
            .expect("send Start with one configured relay");

        // Helper: does this frame's relay_statuses show the configured relay as
        // "connecting"? Match on the host part so trailing-slash / canonical
        // differences do not cause a false miss.
        let needle = "relay.v600.test";
        let connecting_state = |env: &SnapshotEnvelope| -> Option<bool> {
            env.relay_statuses
                .iter()
                .find(|r| r.relay_url.contains(needle))
                .map(|r| r.connection == "connecting")
        };

        // Collect frames within the existing 500 ms D1 budget.
        let frames = drain_snapshots(&upd_rx, Duration::from_millis(500));

        // The FIRST running=true frame is the Start-arm `emit_now` output.
        let first_running = frames
            .iter()
            .find(|s| s.running)
            .expect("#600: a running=true frame must arrive within 500 ms");

        // D1 / #600: the configured relay must NOT yet be "connecting" in the
        // first rendered frame — emit_now ran before spawn_missing_relays dialed
        // it. (The row may be absent or in a pre-dial state, but never
        // "connecting".)
        assert_ne!(
            connecting_state(first_running),
            Some(true),
            "#600: the first running=true frame must be emitted BEFORE the \
             configured relay is dialed — its relay_statuses row must not read \
             \"connecting\" (D1: the first rendered frame is independent of \
             relay I/O). relay_statuses={:?}",
            first_running.relay_statuses
        );

        // Non-vacuous guard: SOME later frame MUST show the relay as
        // "connecting", proving spawn_missing_relays actually dialed it. If it
        // never connects in any frame, the ordering assertion above would be
        // meaningless (no dial ever happened).
        let dial_observed = frames
            .iter()
            .any(|s| connecting_state(s) == Some(true));
        assert!(
            dial_observed,
            "#600: spawn_missing_relays must dial the one configured relay — \
             some frame must show it as \"connecting\"; otherwise the \
             emit-before-dial ordering is untested. frames={}",
            frames.len()
        );

        let _ = cmd_tx.send(ActorCommand::Shutdown);
    }
}
