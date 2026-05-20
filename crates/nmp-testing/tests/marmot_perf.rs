//! Marmot performance measurement harness.
//!
//! Measures the three exit-gate perf numbers from marmot-mls.md §"Exit gate (perf)":
//!   1. GroupMessages cold render: 10 members, 100 messages — get_messages latency.
//!   2. SendMessage local: create_message (encrypt) + process_message (decrypt).
//!   3. InviteMember local: add_members → wrap_welcome → unwrap_and_process_welcome
//!      → accept_welcome → post-join self_update.
//!
//! Relay-network I/O is excluded; these are compute-only timings (MDK
//! operations, SQLite writes, MLS crypto). The `--nocapture` flag is required
//! to see timing output.
//!
//! Run:
//!   cargo test -p nmp-testing --test marmot_perf -- --nocapture

#[path = "marmot_harness.rs"]
mod harness;

use mdk_core::prelude::{MessageProcessingResult, NostrGroupConfigData};
use nostr::{EventBuilder, Keys, Kind, RelayUrl};
use std::time::{Duration, Instant};

fn test_relays() -> Vec<RelayUrl> {
    vec![RelayUrl::parse("wss://test.relay").unwrap()]
}

fn group_config(name: &str, admin_key: &Keys) -> NostrGroupConfigData {
    NostrGroupConfigData::new(
        name.to_string(),
        "perf".to_string(),
        None, None, None,
        test_relays(),
        vec![admin_key.public_key()],
    )
}

fn percentile(mut samples: Vec<Duration>, p: f64) -> Duration {
    samples.sort();
    let idx = ((samples.len() as f64 * p / 100.0) as usize).min(samples.len() - 1);
    samples[idx]
}

// ─── Perf 1: GroupMessages cold render (10 members, 100 messages) ─────────────

#[test]
fn perf_group_messages_cold_render_10_members_100_msgs() {
    const N_MEMBERS: usize = 10;
    const N_MSGS: usize = 100;
    const N_RUNS: usize = 5;

    println!("\n=== Perf 1: GroupMessages cold render ({N_MEMBERS} members, {N_MSGS} msgs) ===");
    println!("Methodology: build group with {N_MEMBERS} members, send {N_MSGS} messages from");
    println!("admin, then call get_messages() (cold = first call after constructing service");
    println!("from storage). Relay-network legs excluded; compute + SQLite I/O only.");

    // ── Build the group with N_MEMBERS members ────────────────────────────────
    let admin_keys = Keys::generate();
    let admin_dir = harness::TestDir::new();
    let admin = harness::service_at(&admin_dir.db_path("admin"), admin_keys.clone());

    // Collect member keys + services.
    let member_dirs: Vec<_> = (0..N_MEMBERS).map(|_| harness::TestDir::new()).collect();
    let member_keys: Vec<_> = (0..N_MEMBERS).map(|_| Keys::generate()).collect();
    let members: Vec<_> = member_keys
        .iter()
        .zip(member_dirs.iter())
        .enumerate()
        .map(|(i, (k, d))| harness::service_at(&d.db_path(&format!("m{i}")), k.clone()))
        .collect();

    // Collect all member key packages.
    let kp_events: Vec<_> = members
        .iter()
        .map(|m| {
            m.publish_key_package(test_relays())
                .expect("member publish kp")
                .event_30443
        })
        .collect();

    // Admin creates group with all members at once.
    let (group, pending) = admin
        .create_group(kp_events, group_config("perf-render", &admin_keys))
        .expect("admin create_group");
    let group_id = group.mls_group_id.clone();

    // Gift-wrap and deliver welcomes.
    for (rumor, (mk, ms)) in pending
        .welcome_rumors
        .clone()
        .into_iter()
        .zip(member_keys.iter().zip(members.iter()))
    {
        let gift = admin
            .wrap_welcome(&mk.public_key(), rumor, None)
            .expect("gift wrap");
        let (w, _) = ms.unwrap_and_process_welcome(&gift).expect("unwrap welcome");
        ms.accept_welcome(&w).expect("accept welcome");
    }
    pending.commit().expect("admin merge create");

    // Post-join self_updates: each member self-updates; admin processes all.
    for (i, (_mk, ms)) in member_keys.iter().zip(members.iter()).enumerate() {
        let su = ms.self_update(&group_id).expect("member self_update");
        let su_ev = su.evolution_event.clone();
        su.commit().expect("member merge su");
        // Admin processes
        let _ = admin.process_message(&su_ev);
        // All other members also need to process to stay in sync.
        // For the perf test we only strictly need admin to process.
        for (j, ms2) in members.iter().enumerate() {
            if i != j {
                let _ = ms2.process_message(&su_ev);
            }
        }
    }

    // Send N_MSGS messages from admin.
    for i in 0..N_MSGS {
        let rumor = EventBuilder::new(Kind::TextNote, format!("msg-{i}"))
            .build(admin_keys.public_key());
        let ev = admin.create_message(&group_id, rumor).expect("create_message");
        // Each member must process to build history in their local store.
        for ms in &members {
            let _ = ms.process_message(&ev);
        }
    }

    // ── Cold render timing ────────────────────────────────────────────────────
    // "Cold" in our context: the first get_messages() call on a member service
    // (the service was constructed at the start of the test, all messages
    // processed during setup — but get_messages has not been called yet).
    let target_member = &members[0];

    let mut latencies = Vec::with_capacity(N_RUNS);
    for _ in 0..N_RUNS {
        let t = Instant::now();
        let history = target_member
            .get_messages(&group_id)
            .expect("get_messages");
        let elapsed = t.elapsed();
        assert_eq!(history.len(), N_MSGS, "history must have {N_MSGS} messages");
        latencies.push(elapsed);
    }

    let p50 = percentile(latencies.clone(), 50.0);
    let p95 = percentile(latencies.clone(), 95.0);
    let min = latencies.iter().min().copied().unwrap();
    let max = latencies.iter().max().copied().unwrap();

    println!("\nResults ({N_RUNS} runs, debug build):");
    println!("  p50  = {:?}", p50);
    println!("  p95  = {:?}", p95);
    println!("  min  = {:?}", min);
    println!("  max  = {:?}", max);
    println!("  target: <= 200 ms (exit gate)");

    // The exit gate says <= 200 ms. In debug builds MDK crypto is slower;
    // we record the actual numbers. Release-mode timings are in the perf report.
    println!("\n[NOTE] Debug-mode timing. See docs/perf/marmot/perf-measurements.md for release numbers.");
}

// ─── Perf 2: SendMessage round-trip (encrypt + local decrypt) ─────────────────

#[test]
fn perf_send_message_encrypt_local_roundtrip() {
    const N_RUNS: usize = 20;

    println!("\n=== Perf 2: SendMessage encrypt→local round-trip ===");
    println!("Methodology: create_message (encrypt) on Alice + process_message");
    println!("(decrypt) on Bob. Relay-publish-and-ack leg excluded (no relay).");

    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let alice_dir = harness::TestDir::new();
    let bob_dir = harness::TestDir::new();
    let alice = harness::service_at(&alice_dir.db_path("alice"), alice_keys.clone());
    let bob = harness::service_at(&bob_dir.db_path("bob"), bob_keys.clone());

    let group_id = harness::setup_two_member_group(
        &alice, &alice_keys, &bob, &bob_keys, "perf-send",
    );

    let mut encrypt_latencies = Vec::with_capacity(N_RUNS);
    let mut decrypt_latencies = Vec::with_capacity(N_RUNS);
    let mut roundtrip_latencies = Vec::with_capacity(N_RUNS);

    for i in 0..N_RUNS {
        let rumor = EventBuilder::new(Kind::TextNote, format!("perf-msg-{i}"))
            .build(alice_keys.public_key());

        let rt_start = Instant::now();

        let enc_start = Instant::now();
        let msg_event = alice
            .create_message(&group_id, rumor)
            .expect("create_message");
        let enc_elapsed = enc_start.elapsed();

        let dec_start = Instant::now();
        match bob.process_message(&msg_event).expect("process_message") {
            MessageProcessingResult::ApplicationMessage(_) => {}
            other => panic!("expected ApplicationMessage, got {other:?}"),
        }
        let dec_elapsed = dec_start.elapsed();

        let rt_elapsed = rt_start.elapsed();

        encrypt_latencies.push(enc_elapsed);
        decrypt_latencies.push(dec_elapsed);
        roundtrip_latencies.push(rt_elapsed);
    }

    let enc_p50 = percentile(encrypt_latencies.clone(), 50.0);
    let enc_p95 = percentile(encrypt_latencies.clone(), 95.0);
    let dec_p50 = percentile(decrypt_latencies.clone(), 50.0);
    let dec_p95 = percentile(decrypt_latencies.clone(), 95.0);
    let rt_p50 = percentile(roundtrip_latencies.clone(), 50.0);
    let rt_p95 = percentile(roundtrip_latencies.clone(), 95.0);

    println!("\nResults ({N_RUNS} runs, debug build):");
    println!("  Encrypt (create_message):   p50={enc_p50:?}  p95={enc_p95:?}");
    println!("  Decrypt (process_message):  p50={dec_p50:?}  p95={dec_p95:?}");
    println!("  Round-trip total:            p50={rt_p50:?}  p95={rt_p95:?}");
    println!("  target: <= 500 ms (exit gate, includes relay leg not measured here)");
    println!("\n[NOTE] Debug-mode timing. See docs/perf/marmot/perf-measurements.md for release numbers.");
}

// ─── Perf 3: InviteMember (add_members → welcome → join) ─────────────────────

#[test]
fn perf_invite_member_create_welcome_peer_join() {
    const N_RUNS: usize = 5;

    println!("\n=== Perf 3: InviteMember (add_members → wrap_welcome → join) ===");
    println!("Methodology: add_members → wrap_welcome → unwrap_and_process_welcome");
    println!("→ accept_welcome → post-join self_update. Relay legs excluded.");

    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let alice_dir = harness::TestDir::new();
    let bob_dir = harness::TestDir::new();
    let alice = harness::service_at(&alice_dir.db_path("alice"), alice_keys.clone());
    let bob = harness::service_at(&bob_dir.db_path("bob"), bob_keys.clone());

    // Establish Alice-only group first (create_group with empty member list
    // to avoid counting the initial Welcome in the invite timing).
    // MDK requires at least one member KP for create_group, so we create a
    // solo group and then add Bob explicitly.
    //
    // Actually MDK's create_group requires at least one invitee key package.
    // We create a two-member group first (Alice+Bob), then for each run we
    // add a fresh Carol. This isolates the add_members timing.

    let group_id = harness::setup_two_member_group(
        &alice, &alice_keys, &bob, &bob_keys, "perf-invite-base",
    );

    let mut invite_latencies = Vec::with_capacity(N_RUNS);

    for run in 0..N_RUNS {
        let carol_keys = Keys::generate();
        let carol_dir = harness::TestDir::new();
        let carol = harness::service_at(&carol_dir.db_path("carol"), carol_keys.clone());

        let carol_kp = carol
            .publish_key_package(test_relays())
            .expect("carol publish kp");

        let t = Instant::now();

        // add_members
        let add_pending = alice
            .add_members(&group_id, &[carol_kp.event_30443.clone()])
            .expect("add_members");
        let carol_rumor = add_pending.welcome_rumors[0].clone();
        let add_event = add_pending.evolution_event.clone();

        // wrap_welcome
        let gift = alice
            .wrap_welcome(&carol_keys.public_key(), carol_rumor, None)
            .expect("wrap_welcome");

        // commit the add
        add_pending.commit().expect("merge add");

        // Bob processes the add commit
        let _ = bob.process_message(&add_event);

        // unwrap_and_process_welcome + accept_welcome
        let (carol_welcome, _) = carol
            .unwrap_and_process_welcome(&gift)
            .expect("unwrap welcome");
        carol.accept_welcome(&carol_welcome).expect("accept welcome");

        // post-join self_update (MIP-02 mandatory — included in invite latency)
        let su = carol.self_update(&group_id).expect("carol su");
        let su_ev = su.evolution_event.clone();
        su.commit().expect("carol merge su");
        let _ = alice.process_message(&su_ev);
        let _ = bob.process_message(&su_ev);

        let elapsed = t.elapsed();
        invite_latencies.push(elapsed);
        println!("  Run {}: {:?}", run, elapsed);
    }

    let p50 = percentile(invite_latencies.clone(), 50.0);
    let p95 = percentile(invite_latencies.clone(), 95.0);
    let min = invite_latencies.iter().min().copied().unwrap();
    let max = invite_latencies.iter().max().copied().unwrap();

    println!("\nResults ({N_RUNS} runs, debug build):");
    println!("  p50  = {:?}", p50);
    println!("  p95  = {:?}", p95);
    println!("  min  = {:?}", min);
    println!("  max  = {:?}", max);
    println!("  target: <= 2 s (exit gate, includes relay legs not measured here)");
    println!("\n[NOTE] Debug-mode timing. See docs/perf/marmot/perf-measurements.md for release numbers.");
}
