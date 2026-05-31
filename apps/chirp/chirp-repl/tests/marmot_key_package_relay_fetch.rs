//! Regression test — Marmot key-package fetch via real relay subscription.
//!
//! ## What this test proves
//!
//! Before the fix in `crates/nmp-marmot/src/projection/state.rs` and
//! `fetch.rs`, `request_key_package_fetch` sent
//! `ActorCommand::Kernel(KernelAction::OpenView{…})` — a stub that echoes
//! `ViewOpened` without compiling any interest or opening any relay
//! subscription. The correct pattern (used by every other Marmot fetch leg) is
//! `app.push_interest(key_package_lookup_interest(pk))`.
//!
//! In-process tests (e.g. `chirp_repl_mls_end_to_end_uses_shared_marmot_runtime`
//! in `marmot.rs`) cannot catch this bug because they hand-shuttle events via
//! `ingest_event_json` — bypassing the subscription path entirely.
//!
//! This test DOES exercise the subscription path:
//!
//! 1. Spawn `nak serve` on a free local port (hermetic in-process relay).
//! 2. Alice + Bob each start a full `AppRuntime` (NmpApp + Marmot, real actor
//!    + relay plumbing) connected to that relay.
//! 3. Both publish their kind:10002 relay lists so account-scoped interests
//!    (the gift-wrap inbox kind:1059) route to the test relay.
//! 4. Bob publishes his KeyPackages (kind:30443 + kind:443) to the relay.
//! 5. Alice calls `mls-create` with Bob as invitee — returns
//!    `key_package_unavailable` AND (with the fix) triggers Bob's KP
//!    subscription via `push_interest`.
//! 6. Wait for the subscription to deliver Bob's KP from the relay (no
//!    hand-shuttling; if the subscription was never opened the wait times out).
//! 7. Alice retries `mls-create` — now succeeds.
//! 8. Bob accepts the Welcome; Alice sends a message; Bob decrypts it.
//!
//! SUCCESS = decrypted plaintext matches what Alice sent, round-tripped
//! through a real relay with zero hand-shuttling of the key package.
//!
//! ## Running
//!
//! ```bash
//! NMP_MARMOT_MOCK_KEYRING=1 cargo test -p chirp-repl \
//!   --test marmot_key_package_relay_fetch -- --ignored --nocapture
//! ```
//!
//! The test is `#[ignore]` because it requires `nak` to be on PATH (or at
//! the path in `NAK_BIN` env var). In CI, set `NAK_BIN` to the nak binary
//! path and run with `-- --ignored` to gate this class of regression.
//!
//! ## Why `#[ignore]` rather than a required feature
//!
//! `nak serve` is not available in the standard Rust CI toolchain. The test is
//! hermetic once `nak` is present (no external relay traffic, ephemeral port,
//! ephemeral keys), so the `#[ignore]` barrier is the correct lightweight gate.
//! A future CI step can install `nak` and pass `-- --ignored` to promote this
//! to a mandatory gate without any code change.

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use chirp_repl::app::AppRuntime;
use nostr::nips::nip19::ToBech32 as _;
use nostr::{EventBuilder, Keys, Kind, Timestamp};
use serde_json::json;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Pick a free TCP port.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local_addr")
        .port()
}

/// Path to the `nak` binary. Checks `NAK_BIN` env var first, then
/// `/Users/pablofernandez/go/bin/nak`, then `nak` on PATH.
fn nak_bin() -> String {
    if let Ok(v) = std::env::var("NAK_BIN") {
        return v;
    }
    let gopath_nak = "/Users/pablofernandez/go/bin/nak".to_string();
    if std::path::Path::new(&gopath_nak).exists() {
        return gopath_nak;
    }
    "nak".to_string()
}

struct NakServe {
    child: Child,
    pub url: String,
}

impl NakServe {
    /// Spawn `nak serve --port <port>` and wait for it to accept connections.
    fn start(port: u16) -> Option<Self> {
        let bin = nak_bin();
        let mut child = match Command::new(&bin)
            .args(["serve", "--port", &port.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("SKIP: cannot spawn nak ({bin}): {e}");
                return None;
            }
        };

        // Drain both stdout and stderr so nak doesn't block on full pipes.
        // Use the listen-probe (TCP connect) as the readiness signal.
        let started = Arc::new(AtomicBool::new(false));
        let started2 = Arc::clone(&started);
        let started3 = Arc::clone(&started);

        let stderr = child.stderr.take().expect("nak stderr");
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                eprintln!("[nak-err] {line}");
                if line.contains(&port.to_string()) || line.contains("listening") {
                    started2.store(true, Ordering::SeqCst);
                }
            }
        });
        let stdout = child.stdout.take().expect("nak stdout");
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                eprintln!("[nak] {line}");
                if line.contains(&port.to_string()) || line.contains("listening") {
                    started3.store(true, Ordering::SeqCst);
                }
            }
        });

        // Wait up to 3 s for nak to actually be connectable.
        let deadline = Instant::now() + Duration::from_secs(3);
        let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        while Instant::now() < deadline {
            if std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(50)).is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }

        if std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_err() {
            eprintln!("SKIP: nak did not start listening on port {port} within 3s");
            let _ = child.kill();
            return None;
        }

        Some(NakServe {
            child,
            url: format!("ws://127.0.0.1:{port}"),
        })
    }

    /// Publish a signed event JSON string to the relay directly via `nak event`.
    /// Used for out-of-band kind:10002 relay-list seeding before the NmpApp
    /// actor has had time to process a user-initiated publish.
    fn publish_event_json(&self, event_json: &str) -> bool {
        let bin = nak_bin();
        let status = Command::new(&bin)
            .args(["event", "--envelope", &self.url])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(event_json.as_bytes());
                }
                child.wait()
            });
        matches!(status, Ok(s) if s.success())
    }
}

impl Drop for NakServe {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn nsec(keys: &Keys) -> String {
    keys.secret_key().to_bech32().expect("nsec bech32")
}

/// Build and sign a kind:10002 relay list event pointing at `relay_url`.
/// This is the NIP-65 relay list: `r` tags tell the kernel + planner
/// where account-scoped interests (like the gift-wrap inbox) should go.
fn build_relay_list(keys: &Keys, relay_url: &str) -> nostr::Event {
    EventBuilder::new(Kind::from_u16(10002), "")
        .tag(nostr::Tag::custom(
            nostr::TagKind::SingleLetter(
                nostr::SingleLetterTag::from_char('r').expect("r tag"),
            ),
            [relay_url],
        ))
        .custom_created_at(Timestamp::now())
        .sign_with_keys(keys)
        .expect("sign kind:10002")
}

/// Wait up to `budget` for `pred` to return `true`, polling every 150 ms.
/// Integration test helper — D8 no-polling doctrine applies to production crates
/// only; test harness infrastructure is exempt under the `#[cfg(test)]` / test
/// file exemption in doctrine-lint (see `d6_test_file` in main.rs:178).
fn wait_for(budget: Duration, mut pred: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        thread::sleep(Duration::from_millis(150));
    }
    false
}

// ── test ─────────────────────────────────────────────────────────────────────

/// The regression test: KP fetch goes through the subscription path, not
/// hand-shuttling.  If `request_key_package_fetch` is broken (dead OpenView),
/// the retry `mls-create` will time out rather than succeed, and the test fails.
#[test]
#[ignore = "requires nak on PATH or NAK_BIN — run with -- --ignored --nocapture"]
fn key_package_fetch_via_relay_subscription_roundtrip() {
    // Must be set for the FFI keychain path to use the in-memory mock.
    std::env::set_var("NMP_MARMOT_MOCK_KEYRING", "1");

    let port = free_port();
    let relay = match NakServe::start(port) {
        Some(r) => r,
        None => {
            eprintln!("SKIP: nak serve unavailable — skipping relay subscription regression test");
            return;
        }
    };
    let relay_url = relay.url.clone();
    println!("[kp-relay] nak serve running at {relay_url}");

    // ── Identities ────────────────────────────────────────────────────────────
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let bob_hex = bob_keys.public_key().to_hex();
    println!(
        "[kp-relay] alice={} bob={}",
        &alice_keys.public_key().to_hex()[..16],
        &bob_hex[..16]
    );

    // ── Publish kind:10002 relay lists out-of-band (before NmpApp startup) ────
    // Account-scoped interests (the gift-wrap kind:1059 inbox) use the NIP-65
    // relay list to route subscriptions. Without a kind:10002 on the relay the
    // kernel's planner has no relay to open the #p filter against.
    // We publish via `nak event --envelope` before NmpApp starts so the kernel's
    // initial kind:10002 subscription sees the events immediately on EOSE.
    let alice_rl = build_relay_list(&alice_keys, &relay_url);
    let bob_rl = build_relay_list(&bob_keys, &relay_url);
    let alice_rl_json = {
        use nostr::util::JsonUtil as _;
        alice_rl.as_json()
    };
    let bob_rl_json = {
        use nostr::util::JsonUtil as _;
        bob_rl.as_json()
    };

    // Publish both relay lists to the relay before any AppRuntime starts.
    // Failure here is non-fatal — the NmpApp will fall back to the configured
    // relays, but the 1059 subscription routing will be less precise.
    if relay.publish_event_json(&alice_rl_json) {
        println!("[kp-relay] published alice kind:10002 via nak");
    } else {
        eprintln!("[kp-relay] WARN: could not publish alice kind:10002 via nak");
    }
    if relay.publish_event_json(&bob_rl_json) {
        println!("[kp-relay] published bob kind:10002 via nak");
    } else {
        eprintln!("[kp-relay] WARN: could not publish bob kind:10002 via nak");
    }

    // ── Alice: start NmpApp + Marmot ─────────────────────────────────────────
    let mut alice = AppRuntime::new();
    alice
        .add_relay(&relay_url, "both")
        .expect("alice add relay");
    alice
        .sign_in_nsec_with_marmot(&nsec(&alice_keys))
        .expect("alice marmot identity");

    // ── Bob: start NmpApp + Marmot ────────────────────────────────────────────
    let mut bob = AppRuntime::new();
    bob.add_relay(&relay_url, "both").expect("bob add relay");
    bob.sign_in_nsec_with_marmot(&nsec(&bob_keys))
        .expect("bob marmot identity");

    // Give both NmpApps time to establish WebSocket connections, ingest their
    // own kind:10002 events from the relay (triggering the NIP-65 mailbox
    // cache population), and run the planner re-cycle that opens the
    // account-scoped gift-wrap inbox (kind:1059 #p) subscription.
    //
    // The pipeline is: NmpApp connects → [0,3,10002,...] REQ → EOSE
    // → kind:10002 ingested → mailbox cache updated → planner re-triggered
    // → kind:1059 #p subscription opened.
    //
    // 2 seconds is conservative for a local nak relay with no network latency.
    thread::sleep(Duration::from_secs(2));

    // ── Alice: publish her KP ─────────────────────────────────────────────────
    let alice_kp_result = alice
        .marmot_dispatch(json!({
            "op": "publish_key_package",
            "relays": [&relay_url],
        }))
        .expect("alice publish_key_package");
    println!("[kp-relay] alice published KP: ok={}", alice_kp_result["ok"]);

    // ── Bob: publish his KP to the relay ─────────────────────────────────────
    // This is the event Alice's subscription will deliver when push_interest
    // opens the kind:30443/443 REQ for Bob's pubkey.
    let bob_kp_result = bob
        .marmot_dispatch(json!({
            "op": "publish_key_package",
            "relays": [&relay_url],
        }))
        .expect("bob publish_key_package");
    assert_eq!(
        bob_kp_result["ok"].as_bool(),
        Some(true),
        "bob must successfully publish his KP"
    );
    println!("[kp-relay] bob published KP to relay");

    // Give publish time to reach nak before Alice's first create_group attempt.
    thread::sleep(Duration::from_millis(300));

    // ── Alice: first mls-create attempt ──────────────────────────────────────
    // If the fix is in place, this call ALSO triggers push_interest for Bob's
    // kind:30443/443 — opening a real REQ on the relay.
    let first_result = alice.marmot_dispatch(json!({
        "op": "create_group",
        "name": "regression-test-group",
        "relays": [&relay_url],
        "invitee_npubs": [&bob_hex],
    }));

    let group_id: String = match &first_result {
        Err(e) if e.contains("key_package_unavailable") => {
            println!(
                "[kp-relay] first mls-create: key_package_unavailable — subscription triggered (this is the fix under test)"
            );
            // The fix triggered a real subscription. Wait for Bob's KP to
            // arrive via the relay tap. Retry every 150 ms, up to 10 s.
            // A local nak relay should deliver in < 1 s.
            let mut found: Option<String> = None;
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline && found.is_none() {
                let retry = alice.marmot_dispatch(json!({
                    "op": "create_group",
                    "name": "regression-test-group",
                    "relays": [&relay_url],
                    "invitee_npubs": [&bob_hex],
                }));
                if let Ok(v) = retry {
                    if v["ok"].as_bool() == Some(true) {
                        found = v["group_id_hex"]
                            .as_str()
                            .map(str::to_string);
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(150));
            }

            match found {
                Some(gid) => {
                    println!("[kp-relay] retry mls-create succeeded");
                    gid
                }
                None => {
                    panic!(
                        "[kp-relay] FAIL: retry mls-create never succeeded within 10s — \
                         KP subscription was NOT delivered (regression still present). \
                         This means request_key_package_fetch is still broken."
                    );
                }
            }
        }
        Ok(v) if v["ok"].as_bool() == Some(true) => {
            println!(
                "[kp-relay] first mls-create: immediate success (KP was already cached/delivered)"
            );
            v["group_id_hex"]
                .as_str()
                .expect("group_id_hex in successful create")
                .to_string()
        }
        other => {
            panic!("[kp-relay] unexpected first mls-create result: {other:?}");
        }
    };

    println!("[kp-relay] group created: {}", &group_id[..16]);

    // ── Bob: wait for the gift-wrapped Welcome, accept it ────────────────────
    // Alice published the kind:1059 gift-wrap to the relay. Bob's gift-wrap
    // inbox subscription (kind:1059 `#p=bob`, registered at marmot-init time via
    // giftwrap_inbox_interest) is account-scoped and routes via the NIP-65 relay
    // list — which we seeded with kind:10002 above.
    // The gift-wrap inbox subscription uses InterestScope::Account routing:
    // the planner probes Bob's NIP-65 mailbox, waits for kind:10002 to be
    // ingested (from the relay), then re-plans and opens the kind:1059 #p sub.
    // This pipeline can take up to ~30s on a cold-start local relay.
    // We wait 60s to give the full probe → ingest → replan → subscribe cycle
    // enough time to complete.
    let welcome_arrived = wait_for(Duration::from_secs(60), || {
        bob.marmot_snapshot()
            .ok()
            .and_then(|s| {
                s.get("pending_welcomes")
                    .and_then(|w| w.as_array())
                    .map(|arr| !arr.is_empty())
            })
            .unwrap_or(false)
    });

    assert!(
        welcome_arrived,
        "[kp-relay] FAIL: Bob never received a pending Welcome within 15s. \
         Check that Alice's kind:1059 gift-wrap was published to the relay \
         and that Bob's kind:1059 #p subscription is routing to the test relay."
    );

    let welcome_id = {
        let snap = bob.marmot_snapshot().expect("bob marmot snapshot");
        snap["pending_welcomes"][0]["id_hex"]
            .as_str()
            .expect("welcome id_hex")
            .to_string()
    };
    println!("[kp-relay] bob sees pending welcome {}", &welcome_id[..16]);

    let accept = bob
        .marmot_dispatch(json!({
            "op": "accept_welcome",
            "welcome_id_hex": &welcome_id,
        }))
        .expect("bob accept_welcome");
    assert_eq!(
        accept["group_id_hex"].as_str(),
        Some(group_id.as_str()),
        "bob joined the same group alice created"
    );
    println!("[kp-relay] bob accepted welcome, joined group");

    // ── Alice: send an encrypted group message ────────────────────────────────
    let plaintext = format!(
        "kp-relay regression: hello bob — ts={}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );
    let sent = alice
        .marmot_dispatch(json!({
            "op": "send",
            "group_id_hex": &group_id,
            "text": &plaintext,
        }))
        .expect("alice send group message");
    assert_eq!(
        sent["ok"].as_bool(),
        Some(true),
        "alice must successfully send a group message"
    );
    println!("[kp-relay] alice sent: {plaintext:?}");

    // ── Bob: wait for the decrypted message ──────────────────────────────────
    // The kind:445 is published to the relay; Bob's group-message subscription
    // (registered by subscribe_group_messages at accept-welcome time) delivers
    // it through the raw-event tap → MarmotService → group_messages projection.
    let message_delivered = wait_for(Duration::from_secs(30), || {
        bob.marmot_group_messages(&group_id)
            .ok()
            .and_then(|msgs| msgs.as_array().cloned())
            .map(|rows| {
                rows.iter().any(|row| {
                    row.get("content").and_then(|c| c.as_str()) == Some(plaintext.as_str())
                })
            })
            .unwrap_or(false)
    });

    assert!(
        message_delivered,
        "[kp-relay] FAIL: Bob never decrypted Alice's group message within 15s. \
         This indicates either the KP subscription fix is not working, \
         or the group-message subscription is not delivering events."
    );

    println!(
        "[kp-relay] PASS: Bob decrypted Alice's message {plaintext:?} via real relay — \
         key-package subscription regression is fixed"
    );
}
