//! NIP-60 wallet CLI — use your own nsec, two-sided.
//!
//! Run in one terminal as Alice (sender), another as Bob (receiver).
//!
//! # Examples
//!
//!   # Check balance (loads existing wallet from relay, or creates one)
//!   wallet-poc --nsec nsec1... balance
//!
//!   # Deposit 100 sat (prints bolt11; testnut auto-pays it)
//!   wallet-poc --nsec nsec1... deposit 100
//!
//!   # Send 50 sat nutzap to a pubkey
//!   wallet-poc --nsec nsec1alice... send <bob-npub-or-hex> 50
//!
//!   # Watch for incoming nutzaps and redeem them (Ctrl-C to stop)
//!   wallet-poc --nsec nsec1bob... receive

use std::time::Duration;

use clap::{Parser, Subcommand};
use nostr::{Filter, Keys, Kind, PublicKey, SecretKey};
use nmp_nip60::{
    decode_nutzap_event, error::Nip60Error, verify_nutzap_dleq, Nip60WalletHandle, KIND_NUTZAP,
};
use tracing::warn;

const DEFAULT_MINT: &str = "https://testnut.cashu.space";

#[derive(Parser)]
#[command(name = "wallet-poc", about = "NIP-60 Cashu wallet CLI")]
struct Cli {
    /// Your Nostr secret key (nsec1… or hex). Omit to generate a throwaway key.
    /// Can also be set via NOSTR_NSEC env var.
    #[arg(long)]
    nsec: Option<String>,

    /// Relay URL to use for your own wallet events (kind:17375, kind:7375, kind:7376).
    /// Recipient discovery uses purplepag.es → their NIP-65 relays automatically.
    #[arg(long)]
    relay: String,

    /// Cashu mint URL. Only used when creating a brand-new wallet.
    #[arg(long, default_value = DEFAULT_MINT)]
    mint: String,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show current wallet balance.
    Balance,

    /// Initiate a deposit. Prints a bolt11 invoice to pay.
    /// testnut.cashu.space auto-pays the invoice — tokens arrive within a second.
    Deposit {
        /// Amount in satoshis.
        amount: u64,
    },

    /// Send a NutZap to a recipient.
    Send {
        /// Recipient pubkey (npub… or hex).
        to: String,
        /// Amount in satoshis.
        amount: u64,
        /// Optional comment.
        #[arg(short, long)]
        comment: Option<String>,
    },

    /// Poll the relay for incoming NutZaps and redeem them.
    Receive,

    /// Publish your kind:10019 NutZap info event (tells others how to send to you).
    Advertise,
}

fn main() {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "nmp_nip60=info,wallet_poc=info".into()),
        )
        .init();

    // Also accept NOSTR_NSEC env var as a fallback.
    let nsec_opt = cli.nsec.or_else(|| std::env::var("NOSTR_NSEC").ok());
    let keys = resolve_keys(&nsec_opt);
    println!("pubkey: {}", keys.public_key().to_hex());
    println!("relay:  {}", cli.relay);

    let wallet = load_or_create_wallet(&keys, &cli.mint, &cli.relay);

    match cli.cmd {
        Cmd::Balance => {
            println!("balance: {} sat", wallet.balance_sats());
        }

        Cmd::Deposit { amount } => {
            let deposit = wallet.initiate_deposit(amount).unwrap_or_else(|e| {
                eprintln!("error: {e}");
                std::process::exit(1);
            });
            println!("invoice: {}", deposit.bolt11);
            println!("quote:   {}", deposit.quote_id);
            println!();
            println!("Waiting for payment...");

            for attempt in 1..=120 {
                std::thread::sleep(Duration::from_millis(500));
                match wallet.complete_deposit(&deposit) {
                    Ok(sats) => {
                        println!("✓ minted {sats} sat after {attempt} poll(s)");
                        println!("balance: {} sat", wallet.balance_sats());
                        return;
                    }
                    Err(Nip60Error::QuoteNotPaid) => {}
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            eprintln!("error: invoice not paid after 60 s");
            std::process::exit(1);
        }

        Cmd::Send { to, amount, comment } => {
            let recipient = parse_pubkey(&to);
            match wallet.send_nutzap(
                amount,
                &recipient,
                &[cli.relay.clone()],
                comment.as_deref(),
                None,
            ) {
                Ok(id) => {
                    println!("✓ nutzap sent: {id}");
                    println!("balance: {} sat", wallet.balance_sats());
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }

        Cmd::Receive => {
            println!("Watching for nutzaps (Ctrl-C to stop)…");
            let filter = Filter::new()
                .kind(Kind::from(KIND_NUTZAP))
                .pubkey(keys.public_key());

            let relay = cli.relay.clone();
            loop {
                match nmp_nip60::relay::fetch_events(&relay, filter.clone()) {
                    Ok(events) => {
                        for event in events {
                            match decode_nutzap_event(&event) {
                                Ok(nutzap) => {
                                    println!(
                                        "nutzap: {} sat from {} — \"{}\"",
                                        nutzap.amount_sats,
                                        nutzap.sender_pubkey.to_hex(),
                                        nutzap.comment
                                    );
                                    match verify_nutzap_dleq(&nutzap) {
                                        Ok(()) => println!("  ✓ DLEQ verified"),
                                        Err(e) => println!("  ⚠ DLEQ: {e}"),
                                    }
                                    match wallet.redeem_nutzap(&nutzap) {
                                        Ok(sats) => {
                                            println!(
                                                "  ✓ redeemed {sats} sat — balance: {} sat",
                                                wallet.balance_sats()
                                            );
                                        }
                                        Err(e) => {
                                            // Already redeemed proofs cause a swap error — ignore.
                                            warn!("redeem: {e}");
                                        }
                                    }
                                }
                                Err(e) => warn!("decode nutzap: {e}"),
                            }
                        }
                    }
                    Err(e) => warn!("fetch: {e}"),
                }
                std::thread::sleep(Duration::from_secs(3));
            }
        }

        Cmd::Advertise => {
            match wallet.publish_nutzap_info() {
                Ok(id) => println!("✓ kind:10019 published: {id}"),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

// ── helpers ────────────────────────────────────────────────────────────────

fn resolve_keys(nsec: &Option<String>) -> Keys {
    match nsec {
        Some(s) => {
            // Accept nsec1… bech32 or raw hex.
            let sk = if s.starts_with("nsec") {
                SecretKey::parse(s).unwrap_or_else(|e| {
                    eprintln!("invalid nsec: {e}");
                    std::process::exit(1);
                })
            } else {
                SecretKey::from_hex(s).unwrap_or_else(|e| {
                    eprintln!("invalid hex secret key: {e}");
                    std::process::exit(1);
                })
            };
            Keys::new(sk)
        }
        None => {
            let k = Keys::generate();
            println!("(no --nsec given — generated throwaway key)");
            println!("nsec: {}", k.secret_key().to_secret_hex());
            k
        }
    }
}

fn parse_pubkey(s: &str) -> PublicKey {
    PublicKey::parse(s).unwrap_or_else(|e| {
        eprintln!("invalid pubkey '{}': {e}", s);
        std::process::exit(1);
    })
}

/// Load an existing NIP-60 wallet from the relay; create a new one if none found.
fn load_or_create_wallet(keys: &Keys, mint: &str, relay: &str) -> Nip60WalletHandle {
    match Nip60WalletHandle::load_from_relays(keys, &[relay.to_string()]) {
        Ok(w) => {
            println!("(loaded existing wallet — balance {} sat)", w.balance_sats());
            w
        }
        Err(Nip60Error::NotInitialised) => {
            println!("(no existing wallet — creating new one with mint {mint})");
            let w = Nip60WalletHandle::create_new(keys, mint, vec![relay.to_string()])
                .unwrap_or_else(|e| {
                    eprintln!("error creating wallet: {e}");
                    std::process::exit(1);
                });
            // Auto-advertise so others can send to us immediately.
            let _ = w.publish_nutzap_info();
            w
        }
        Err(e) => {
            eprintln!("error loading wallet: {e}");
            std::process::exit(1);
        }
    }
}
