//! #2880 (epic #2864) — real-kernel reactivity proof for the NIP-87
//! discovered-mints view riding the `wallet.merged` typed projection.
//!
//! Unlike the in-crate `nmp-wallet` unit tests (which hand-feed the registered
//! observed-projection sink), this drives events through the REAL ingest/emit
//! path: a composed `NmpApp` (`.with_wallet()`), a signed-in account, three
//! signed discovery events injected via `inject_signed_event_json_for_test`
//! (kernel verify + accept + observed-projection fan-out), and the emitted
//! `wallet.merged` (`NWMP`) sidecar read back through
//! `run_typed_snapshot_projections()` — the exact vector the actor folds into a
//! snapshot frame. It locks in the property the review asked for: a
//! DISCOVERY-ONLY event stream (no wallet action, no wallet-state change)
//! re-emits `wallet.merged` with an updated `discovered_mints`.
#![cfg(all(feature = "wallet", feature = "test-support"))]

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use nmp_core::WireProjectionState;
use nmp_native_runtime::{NmpApp, NmpAppBuilder, RunConfig};
use nmp_wallet::{decode_wallet_projection, WalletProjection, WalletReadiness};
use nostr::{Event, EventBuilder, JsonUtil, Keys, Kind, SecretKey, Tag, Timestamp, ToBech32};

// Kind:38172 mint announcement / kind:38000 mint recommendation (NIP-87).
const KIND_MINT_ANNOUNCE: u16 = 38_172;
const KIND_MINT_RECOMMEND: u16 = 38_000;

static SERIAL: Mutex<()> = Mutex::new(());
static UPDATE_TX: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();

fn signal_update() {
    if let Some(slot) = UPDATE_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(());
            }
        }
    }
}

struct WalletApp {
    app: *mut NmpApp,
    ticks: Receiver<()>,
}

impl WalletApp {
    fn boot() -> Self {
        let builder = NmpAppBuilder::new()
            .with_wallet()
            .expect("with_wallet registers cleanly on a fresh builder");
        let app = builder
            .in_memory()
            .consume_all_builtin_projections()
            .without_initial_relays()
            .start(RunConfig::default());

        let (tx, ticks) = channel::<()>();
        let slot = UPDATE_TX.get_or_init(|| Mutex::new(None));
        *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
        unsafe { &*app }.set_update_listener(Some(std::sync::Arc::new(|_bytes: &[u8]| {
            signal_update();
        })));

        Self { app, ticks }
    }

    fn app_ref(&self) -> &NmpApp {
        unsafe { &*self.app }
    }

    fn sign_in(&self, keys: &Keys) {
        let nsec = keys.secret_key().to_bech32().expect("nsec bech32");
        self.app_ref().signin_nsec_for_test(nsec, true);
        let pubkey = keys.public_key().to_hex();
        wait_for(&self.ticks, "active account", || {
            self.app_ref()
                .active_account_handle()
                .lock()
                .map(|slot| slot.as_deref() == Some(pubkey.as_str()))
                .unwrap_or(false)
        });
    }

    fn inject(&self, event: &Event) {
        assert!(
            self.app_ref().inject_signed_event_json_for_test(&event.as_json()),
            "signed event must verify and inject"
        );
        assert!(
            self.app_ref()
                .wait_barrier_for_test(Duration::from_millis(5_000)),
            "actor must process the injected event before the test continues"
        );
    }

    /// Decode the currently-emitted `wallet.merged` (`NWMP`) sidecar, or `None`
    /// when the projection has not emitted a live row yet.
    fn wallet_merged(&self) -> Option<WalletProjection> {
        let row = self
            .app_ref()
            .run_typed_snapshot_projections()
            .into_iter()
            .find(|row| {
                row.key == nmp_wallet::WALLET_MERGED_PROJECTION_KEY
                    && row.state != WireProjectionState::Cleared
            })?;
        Some(decode_wallet_projection(&row.payload).expect("NWMP payload decodes"))
    }
}

impl Drop for WalletApp {
    fn drop(&mut self) {
        self.app_ref().set_update_listener(None);
        if let Some(slot) = UPDATE_TX.get() {
            if let Ok(mut guard) = slot.lock() {
                *guard = None;
            }
        }
        unsafe {
            self.app_ref().stop_runtime();
            drop(Box::from_raw(self.app));
        }
    }
}

fn wait_for(rx: &Receiver<()>, label: &str, pred: impl Fn() -> bool) {
    if pred() {
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(remaining.min(Duration::from_secs(1))) {
            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if pred() {
                    return;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    panic!("timed out waiting for {label}");
}

fn keys_from_byte(byte: u8) -> Keys {
    Keys::new(SecretKey::from_slice(&[byte; 32]).expect("valid secret key"))
}

fn signed_contact_list(keys: &Keys, follows: &[String], created_at: u64) -> Event {
    let tags: Vec<Tag> = follows
        .iter()
        .map(|pk| Tag::parse(["p", pk.as_str()]).expect("valid p tag"))
        .collect();
    EventBuilder::new(Kind::from(3u16), "")
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:3")
}

fn signed_announcement(keys: &Keys, d: &str, url: &str, created_at: u64) -> Event {
    EventBuilder::new(Kind::from(KIND_MINT_ANNOUNCE), "")
        .tags([
            Tag::parse(["d", d]).expect("valid d tag"),
            Tag::parse(["u", url]).expect("valid u tag"),
            Tag::parse(["nuts", "1,2,4,7,11,12"]).expect("valid nuts tag"),
            Tag::parse(["name", "Reactivity Mint"]).expect("valid name tag"),
        ])
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:38172")
}

fn signed_recommendation(keys: &Keys, url: &str, created_at: u64) -> Event {
    EventBuilder::new(Kind::from(KIND_MINT_RECOMMEND), "")
        .tags([
            Tag::parse(["k", "38172"]).expect("valid k tag"),
            Tag::parse(["u", url]).expect("valid u tag"),
        ])
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:38000")
}

#[test]
fn discovery_only_events_reemit_wallet_merged_with_updated_discovered_mints() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());

    let viewer = keys_from_byte(0xA1);
    let recommender = keys_from_byte(0xB2);
    let mint_url = "https://reactivity.mint";

    let app = WalletApp::boot();
    app.sign_in(&viewer);

    // Baseline: a signed-in-but-quiet wallet emits a live `wallet.merged` row
    // with NO discovered mints and NO wallet state.
    let baseline = app.wallet_merged().expect("wallet.merged emits once signed in");
    assert!(
        baseline.discovered_mints.is_empty(),
        "no discovery events yet -> empty discovered_mints"
    );
    assert_eq!(baseline.readiness, WalletReadiness::NotConfigured);
    assert!(baseline.balances.is_empty());

    // Drive the discovery-only triplet through the REAL ingest lane:
    //   1. viewer follows the recommender (kind:3) -> WoT direct-follow edge,
    //   2. recommender announces a nutzap-capable mint (kind:38172),
    //   3. recommender vouches for it (kind:38000).
    // None of these is a wallet action; none changes wallet state.
    app.inject(&signed_contact_list(
        &viewer,
        &[recommender.public_key().to_hex()],
        1_000,
    ));
    app.inject(&signed_announcement(&recommender, "mint-d", mint_url, 1_001));
    app.inject(&signed_recommendation(&recommender, mint_url, 1_002));

    // The registered `wallet.merged` emitter must now surface the discovered
    // mint — proving the discovery view reaches the FFI projection reactively,
    // driven only by ingested NIP-87 events.
    wait_for(&app.ticks, "discovered_mints populated", || {
        app.wallet_merged()
            .is_some_and(|projection| !projection.discovered_mints.is_empty())
    });

    let projection = app.wallet_merged().expect("wallet.merged still emits");
    assert_eq!(projection.discovered_mints.len(), 1);
    let mint = &projection.discovered_mints[0];
    assert_eq!(mint.url, mint_url);
    assert_eq!(mint.name.as_deref(), Some("Reactivity Mint"));
    assert!(mint.supports_nutzap);
    assert_eq!(mint.recommendation_count, 1);
    assert!(
        mint.trust_score >= 100,
        "a direct-follow recommender contributes at least DIRECT_FOLLOW_SCORE (100), got {}",
        mint.trust_score
    );

    // The discovery-only stream must NOT have moved wallet state — same
    // readiness / empty balances as the baseline, proving this is a pure
    // discovery re-emit and not a side effect of wallet activity.
    assert_eq!(projection.readiness, WalletReadiness::NotConfigured);
    assert!(projection.balances.is_empty());
}
