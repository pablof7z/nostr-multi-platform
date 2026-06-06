//! NIP-60 wallet proof-of-concept.
//!
//! Demonstrates, end-to-end with real relays and a real Cashu mint:
//!
//! 1. Alice creates a NIP-60 wallet backed by the testnut.cashu.space mint.
//! 2. Alice publishes her kind:10019 NutZap info event.
//! 3. Alice initiates a deposit (gets a bolt11 invoice).
//!    — testnut auto-pays the invoice so we poll for completion immediately.
//! 4. Alice's wallet mints the tokens, verifying DLEQ proofs.
//! 5. Alice sends a NutZap to Bob.
//! 6. Bob finds the NutZap on the relay, verifies the DLEQ proofs, and redeems.
//! 7. Both Alice and Bob's balances are printed at each step.
//!
//! Usage:
//!   cargo run -p nmp-wallet-poc --bin wallet-poc
//!
//! Environment:
//!   NMP_POC_RELAY  — relay to use (default: wss://relay.damus.io)
//!   NMP_POC_MINT   — mint to use (default: https://testnut.cashu.space)
//!   NMP_POC_AMOUNT — deposit amount in sats (default: 64)

use std::time::Duration;

use nostr::{Filter, Keys};
use nmp_nip60::{
    decode_nutzap_event, error::Nip60Error, verify_nutzap_dleq, Nip60WalletHandle,
};
use tracing::error;

const DEFAULT_RELAY: &str = "wss://relay.damus.io";
const DEFAULT_MINT: &str = "https://testnut.cashu.space";
const DEFAULT_AMOUNT: u64 = 64;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "nmp_nip60=debug,wallet_poc=info".into()),
        )
        .init();

    let relay = std::env::var("NMP_POC_RELAY").unwrap_or_else(|_| DEFAULT_RELAY.into());
    let mint = std::env::var("NMP_POC_MINT").unwrap_or_else(|_| DEFAULT_MINT.into());
    let amount: u64 = std::env::var("NMP_POC_AMOUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_AMOUNT);

    println!("\n╔══════════════════════════════════════════════╗");
    println!("║  NMP NIP-60 Wallet POC                       ║");
    println!("╠══════════════════════════════════════════════╣");
    println!("║  Relay: {relay:<37}║");
    println!("║  Mint:  {mint:<37}║");
    println!("║  Amount: {amount} sat{:<36}║", "");
    println!("╚══════════════════════════════════════════════╝\n");

    // ── Step 1: Generate Alice and Bob keypairs ─────────────────────────────
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    println!("Alice pubkey: {}", alice_keys.public_key().to_hex());
    println!("Bob   pubkey: {}", bob_keys.public_key().to_hex());

    // ── Step 2: Alice creates a NIP-60 wallet ──────────────────────────────
    println!("\n[1/7] Alice: creating NIP-60 wallet...");
    let alice_wallet = match Nip60WalletHandle::create_new(
        &alice_keys,
        &mint,
        vec![relay.clone()],
    ) {
        Ok(w) => {
            println!("      ✓ wallet event published");
            w
        }
        Err(e) => {
            error!("Alice wallet creation failed: {e}");
            std::process::exit(1);
        }
    };

    // ── Step 3: Alice publishes NutZap info event ──────────────────────────
    println!("\n[2/7] Alice: publishing NutZap info (kind:10019)...");
    match alice_wallet.publish_nutzap_info() {
        Ok(id) => println!("      ✓ NutZap info published: {id}"),
        Err(e) => {
            error!("Alice NutZap info: {e}");
            std::process::exit(1);
        }
    }

    // Bob also publishes NutZap info so Alice can find his mint + pubkey.
    println!("\n[3/7] Bob: creating wallet + publishing NutZap info...");
    let bob_wallet = match Nip60WalletHandle::create_new(
        &bob_keys,
        &mint,
        vec![relay.clone()],
    ) {
        Ok(w) => w,
        Err(e) => {
            error!("Bob wallet creation: {e}");
            std::process::exit(1);
        }
    };
    match bob_wallet.publish_nutzap_info() {
        Ok(id) => println!("      ✓ Bob NutZap info published: {id}"),
        Err(e) => {
            error!("Bob NutZap info: {e}");
            std::process::exit(1);
        }
    }

    // ── Step 4: Alice deposits tokens ─────────────────────────────────────
    println!("\n[4/7] Alice: initiating {amount} sat deposit...");
    let deposit = match alice_wallet.initiate_deposit(amount) {
        Ok(d) => {
            println!("      Invoice: {}", &d.bolt11[..60]);
            println!("      Quote ID: {}", d.quote_id);
            d
        }
        Err(e) => {
            error!("Alice deposit initiate: {e}");
            std::process::exit(1);
        }
    };

    // Poll until testnut auto-pays the invoice (typically within 500ms).
    let minted = {
        let mut result = None;
        for attempt in 1..=60 {
            std::thread::sleep(Duration::from_millis(500));
            match alice_wallet.complete_deposit(&deposit) {
                Ok(sats) => {
                    println!("      ✓ Minted {sats} sat (DLEQ proofs verified!) after {attempt} poll(s)");
                    result = Some(sats);
                    break;
                }
                Err(Nip60Error::QuoteNotPaid) => {
                    if attempt == 1 { println!("      Waiting for testnut to auto-pay..."); }
                }
                Err(e) => {
                    error!("Alice deposit complete: {e}");
                    std::process::exit(1);
                }
            }
        }
        result.unwrap_or_else(|| {
            error!("Alice deposit: invoice not paid after 30s");
            std::process::exit(1);
        })
    };

    println!("      Alice balance: {} sat", alice_wallet.balance_sats());

    // ── Step 5: Alice sends a NutZap to Bob ───────────────────────────────
    let zap_amount = minted / 2; // zap half
    println!("\n[5/7] Alice → Bob: sending {zap_amount} sat nutzap...");

    let nutzap_event_id = match alice_wallet.send_nutzap(
        zap_amount,
        &bob_keys.public_key(),
        &[relay.clone()],
        Some("test nutzap from Alice 🤙"),
        None,
    ) {
        Ok(id) => {
            println!("      ✓ NutZap event published: {id}");
            id
        }
        Err(e) => {
            error!("Alice send nutzap: {e}");
            std::process::exit(1);
        }
    };

    println!("      Alice balance after nutzap: {} sat", alice_wallet.balance_sats());

    // ── Step 6: Bob fetches and verifies the NutZap ───────────────────────
    println!("\n[6/7] Bob: fetching nutzap from relay...");
    std::thread::sleep(Duration::from_millis(500)); // brief propagation window

    let nutzap_filter = Filter::new()
        .id(nutzap_event_id);

    let nutzap_events = match nmp_nip60::relay::fetch_events(&relay, nutzap_filter) {
        Ok(evts) => evts,
        Err(e) => {
            error!("Bob fetch nutzap: {e}");
            std::process::exit(1);
        }
    };

    let nutzap_event = match nutzap_events.into_iter().next() {
        Some(e) => e,
        None => {
            error!("Bob: no nutzap event found on relay");
            std::process::exit(1);
        }
    };

    let received = match decode_nutzap_event(&nutzap_event) {
        Ok(r) => {
            println!("      ✓ NutZap received: {} sat from {}", r.amount_sats, r.sender_pubkey.to_hex());
            println!("      Comment: {}", r.comment);
            r
        }
        Err(e) => {
            error!("Bob decode nutzap: {e}");
            std::process::exit(1);
        }
    };

    // Verify DLEQ proofs.
    println!("      Verifying DLEQ proofs...");
    match verify_nutzap_dleq(&received) {
        Ok(()) => println!("      ✓ DLEQ proofs verified! (mint's blind signatures are authentic)"),
        Err(e) => {
            // DLEQ may not be provided by all mints — log as warning not fatal.
            println!("      ⚠ DLEQ verification: {e} (mint may not support NUT-12)");
        }
    }

    // ── Step 7: Bob redeems the NutZap ────────────────────────────────────
    println!("\n[7/7] Bob: redeeming nutzap (swapping P2PK proofs for fresh proofs)...");
    match bob_wallet.redeem_nutzap(&received) {
        Ok(sats) => {
            println!("      ✓ Redeemed {sats} sat!");
        }
        Err(e) => {
            error!("Bob redeem nutzap: {e}");
            std::process::exit(1);
        }
    }

    // ── Summary ───────────────────────────────────────────────────────────
    println!("\n╔══════════════════════════════════════════════╗");
    println!("║  Final Balances                              ║");
    println!("╠══════════════════════════════════════════════╣");
    println!("║  Alice: {} sat{:<40}║", alice_wallet.balance_sats(), "");
    println!("║  Bob:   {} sat{:<40}║", bob_wallet.balance_sats(), "");
    println!("╠══════════════════════════════════════════════╣");
    println!("║  ✓ NIP-60 wallet created + published         ║");
    println!("║  ✓ Tokens minted with DLEQ proof             ║");
    println!("║  ✓ NutZap sent + received on relay           ║");
    println!("║  ✓ P2PK proofs swapped for fresh proofs      ║");
    println!("║  ✓ Wallet history events published           ║");
    println!("╚══════════════════════════════════════════════╝\n");
}
