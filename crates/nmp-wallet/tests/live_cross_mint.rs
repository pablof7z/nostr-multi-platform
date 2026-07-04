//! LIVE cross-mint transfer integration test (#3003) — network-gated behind
//! `#[ignore]`. Melts a small amount at one real mint to fund another via
//! the actual `WalletIntent::CrossMintTransfer` saga, and asserts the
//! target mint's balance appears in the wallet's projection.
//!
//! Offline in CI (ignored by default, never invoked by `cargo test` without
//! `-- --ignored`). Run explicitly:
//!   NMP_CROSSMINT_LIVE=1 \
//!   NMP_CROSSMINT_SOURCE_MINT=https://testnut.cashu.space \
//!   NMP_CROSSMINT_TARGET_MINT=https://<a-second-real-mint> \
//!   cargo test -p nmp-wallet --test live_cross_mint -- --ignored --nocapture
//!
//! Both mint URLs are read from the environment (no hardcoded second mint —
//! `testnut.cashu.space` is this repo's only universally-available public
//! test mint; a genuine cross-mint proof requires TWO real, NUT-04/NUT-05-
//! capable mints, with the source able to actually route a Lightning
//! payment to the target's invoice). `NMP_CROSSMINT_SOURCE_MINT` defaults
//! to `testnut.cashu.space` (auto-settles deposits, so funding the source
//! leg needs no manual invoice payment); `NMP_CROSSMINT_TARGET_MINT` has NO
//! default — the operator running this test must supply a real second mint.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use nmp_core::actor::{ActionLedgerCommand, ActorCommand};
use nmp_core::substrate::{
    ActionStageTracker, CachedEventLookup, EmptyDmInboxRelayLookup, KernelClock, KernelEvent,
    NoopErrorSurface, NoopHostOpHandlerAccess, NoopLocalSignerAccess, NoopWalletKernelAccess,
    NoopZapProfileLookup, ProtocolCommand, ProtocolCommandContext, ProtocolCommandContextParts,
    RecipientRelayLookup,
};
use nmp_wallet::{CashuWalletBackend, WalletBackend, WalletBackendContext, WalletIntent};

const ACCOUNT: &str = "d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4";
const AMOUNT_SATS: u64 = 5;

struct FixedClock(u64);
impl KernelClock for FixedClock {
    fn now_secs(&self) -> u64 {
        self.0
    }
}

struct EmptyRecipientLookup;
impl RecipientRelayLookup for EmptyRecipientLookup {
    fn recipient_publish_relays(&self, _recipient: &str, _kind: u32) -> Vec<String> {
        Vec::new()
    }
}

struct NoopStages;
impl ActionStageTracker for NoopStages {
    fn record_requested(&self, _correlation_id: &str) {}
}

#[derive(Default)]
struct EmptyCachedEvents;
impl CachedEventLookup for EmptyCachedEvents {
    fn event_by_id(&self, _id: &str) -> Option<KernelEvent> {
        None
    }
    fn latest_author_kind(&self, _author: &str, _kind: u32) -> Option<KernelEvent> {
        None
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs()
}

fn ctx_for(account: &str, now: u64) -> WalletBackendContext<'_> {
    WalletBackendContext {
        now_secs: now,
        selected_backend: None,
        account_pubkey: Some(account),
    }
}

/// Run one `ProtocolCommand` to completion (its own real mint HTTP happens
/// on a spawned worker thread, D8), draining every `ActorMail::Command` it
/// sends back for up to `timeout` — real wall-clock time, since this test
/// drives genuine network I/O, not kernel/actor state (D8 is about never
/// polling library code on the actor thread; this is a plain integration
/// test driving a real command against real mints).
fn run_and_collect(cmd: Box<dyn ProtocolCommand>, timeout: Duration) -> Vec<ActorCommand> {
    static SIGNERS: NoopLocalSignerAccess = NoopLocalSignerAccess;
    static EMPTY_DM: EmptyDmInboxRelayLookup = EmptyDmInboxRelayLookup;
    static ERRORS: NoopErrorSurface = NoopErrorSurface;
    static HOST_OP: NoopHostOpHandlerAccess = NoopHostOpHandlerAccess;
    static WALLET: NoopWalletKernelAccess = NoopWalletKernelAccess;
    static ZAP: NoopZapProfileLookup = NoopZapProfileLookup;
    static STAGES: NoopStages = NoopStages;

    let sink_send = |_c: ActorCommand| {};
    let clock = FixedClock(now_secs());
    let recipients = EmptyRecipientLookup;
    let cached = EmptyCachedEvents;
    let (worker_tx, worker_rx) = mpsc::channel::<nmp_core::ActorMail>();
    {
        let mut c = ProtocolCommandContext::new(ProtocolCommandContextParts {
            send: &sink_send,
            command_sender: nmp_core::CommandSender::new(worker_tx),
            clock: &clock,
            signers: &SIGNERS,
            dms: &EMPTY_DM,
            errors: &ERRORS,
            stages: &STAGES,
            recipients: &recipients,
            host_op_handler: &HOST_OP,
            wallet_kernel: &WALLET,
            zap_profiles: &ZAP,
        })
        .with_cached_events(&cached);
        cmd.run(&mut c).expect("run() itself never fails");
    }

    let deadline = Instant::now() + timeout;
    let mut collected = Vec::new();
    while let Ok(remaining) = deadline.checked_duration_since(Instant::now()).ok_or(()) {
        if remaining.is_zero() {
            break;
        }
        match worker_rx.recv_timeout(remaining) {
            Ok(nmp_core::ActorMail::Command(cmd)) => collected.push(cmd),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    collected
}

/// Pull the `quote_id` a `CashuDepositQuoteCommand`'s `RecordSuccess` result
/// JSON carries (see `deposit/quote.rs`'s `{"quote_id", "bolt11", "mint",
/// "amount_sats"}` shape).
fn extract_quote_id(commands: &[ActorCommand]) -> Option<String> {
    commands.iter().find_map(|c| match c {
        ActorCommand::ActionLedger(ActionLedgerCommand::RecordSuccess {
            result_json: Some(json),
            ..
        }) => serde_json::from_str::<serde_json::Value>(json)
            .ok()?
            .get("quote_id")?
            .as_str()
            .map(str::to_string),
        _ => None,
    })
}

#[test]
#[ignore = "live network + real mints — run with `NMP_CROSSMINT_LIVE=1 NMP_CROSSMINT_TARGET_MINT=<mint> cargo test -p nmp-wallet --test live_cross_mint -- --ignored --nocapture`"]
fn melt_at_source_funds_target_mint_balance() {
    if std::env::var("NMP_CROSSMINT_LIVE").as_deref() != Ok("1") {
        eprintln!("skipping: set NMP_CROSSMINT_LIVE=1 to run this live test");
        return;
    }
    let source_mint = std::env::var("NMP_CROSSMINT_SOURCE_MINT")
        .unwrap_or_else(|_| "https://testnut.cashu.space".to_string());
    let target_mint = std::env::var("NMP_CROSSMINT_TARGET_MINT").unwrap_or_else(|_| {
        panic!(
            "NMP_CROSSMINT_TARGET_MINT must name a second, real NUT-04/NUT-05-capable mint — \
             there is no safe default second mint to assume"
        )
    });

    let backend = CashuWalletBackend::new();

    // 1. Create the wallet at the SOURCE mint.
    for c in backend.start_intent(
        ctx_for(ACCOUNT, now_secs()),
        WalletIntent::CreateCashuWallet {
            mint: source_mint.clone(),
        },
        None,
    ) {
        if let ActorCommand::Protocol(cmd) = c {
            let _ = run_and_collect(cmd, Duration::from_secs(10));
        }
    }

    // 2. Request a deposit quote for headroom over the transfer amount
    // (covers the melt's fee reserve).
    let deposit_amount = AMOUNT_SATS + 10;
    let mut quote_id = None;
    for c in backend.start_intent(
        ctx_for(ACCOUNT, now_secs()),
        WalletIntent::DepositQuoteCashu {
            mint: source_mint.clone(),
            amount_sats: deposit_amount,
        },
        Some("live-cross-mint-deposit-quote".to_string()),
    ) {
        if let ActorCommand::Protocol(cmd) = c {
            let sent = run_and_collect(cmd, Duration::from_secs(15));
            quote_id = extract_quote_id(&sent);
        }
    }
    let quote_id = quote_id.expect(
        "expected a quote_id from CashuDepositQuoteCommand's RecordSuccess — \
         did the source mint's /v1/mint/quote/bolt11 request fail?",
    );

    // 3. Poll `CompleteDepositCashu` until the source mint reports the
    // quote PAID (testnut auto-settles almost immediately; a real mint
    // needs the printed invoice paid out-of-band by the operator within
    // this window).
    let deposit_deadline = Instant::now() + Duration::from_secs(120);
    let mut deposited = false;
    while Instant::now() < deposit_deadline && !deposited {
        for c in backend.start_intent(
            ctx_for(ACCOUNT, now_secs()),
            WalletIntent::CompleteDepositCashu {
                quote_id: quote_id.clone(),
            },
            Some("live-cross-mint-complete-deposit".to_string()),
        ) {
            if let ActorCommand::Protocol(cmd) = c {
                let _ = run_and_collect(cmd, Duration::from_secs(15));
            }
        }
        let projection = backend
            .snapshot(nmp_wallet::WalletProjectionScope::default())
            .projection;
        deposited = projection
            .balances
            .iter()
            .any(|b| b.mint == source_mint && b.amount >= deposit_amount);
        if !deposited {
            std::thread::sleep(Duration::from_secs(3));
        }
    }
    assert!(
        deposited,
        "source mint {source_mint} never reported the deposit paid within the poll window \
         (pay the printed invoice out-of-band if this isn't testnut)"
    );

    // 4. THE saga under test: fund `target_mint` via a cross-mint transfer.
    for c in backend.start_intent(
        ctx_for(ACCOUNT, now_secs()),
        WalletIntent::CrossMintTransfer {
            target_mint: target_mint.clone(),
            amount_sats: AMOUNT_SATS,
        },
        Some("live-cross-mint-transfer".to_string()),
    ) {
        if let ActorCommand::Protocol(cmd) = c {
            let _ = run_and_collect(cmd, Duration::from_secs(60));
        }
    }

    let projection = backend
        .snapshot(nmp_wallet::WalletProjectionScope::default())
        .projection;
    let has_target_balance = projection
        .balances
        .iter()
        .any(|b| b.mint == target_mint && b.amount > 0);
    assert!(
        has_target_balance,
        "expected a positive balance at {target_mint} after the cross-mint transfer; \
         balances = {:?}",
        projection.balances
    );
}
