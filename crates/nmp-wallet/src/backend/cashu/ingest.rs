//! Wallet-recovery ingestion (#2965, epic #2864) — decode + fold a
//! signer-decrypted kind:17375 (wallet config) / kind:7375 (token) plaintext
//! into [`CashuWalletState`], so an account that already has a wallet
//! published on relays gets it LOADED rather than silently orphaned behind a
//! fresh `CreateCashuWallet`.
//!
//! Two entry points, both pure (no I/O, no port calls) so they are directly
//! unit-testable against a bare `CashuWalletState`:
//!
//! - [`ingest_wallet_config`] — kind:17375: the Cashu privkey + accepted
//!   mints, mirroring what `CreateCashuWalletCommand`'s `on_signed` closure
//!   sets for a FRESH wallet (`create_wallet.rs`'s `wallet_config_plaintext`
//!   is this function's write-side counterpart — same `[[key, value], ...]`
//!   JSON shape). A no-op once `state.created` is already true: never
//!   clobber a wallet that may already hold balance, the same invariant
//!   `start_create_wallet`'s `ALREADY_CREATED` fail-closed check enforces.
//! - [`ingest_token_event`] — kind:7375: proofs, folded through
//!   `state.ledger` (confluence-safe — see below) and mirrored into
//!   `state.proofs` (the secret-bearing store `SendNutzap`/`RedeemNutzap`
//!   actually spend from).
//!
//! Both callers decrypt the event's content through the signer-transparent
//! NIP-44 port (`build_nip44_decrypt_for_account`, D13 — this crate never
//! holds/uses a raw `Keys`/`SecretKey` derived from account key material) and
//! hand the resulting plaintext here; see `mod.rs`'s
//! `on_self_authored_wallet_event` (passive cold-start replay + live tail)
//! and `recover.rs`'s `RecoverCashuWalletCommand` (the explicit
//! `nmp.wallet.cashu.recover` action).
//!
//! # Confluence (kind:7375 supersession, #2965 requirement 3)
//!
//! A NIP-60 wallet rolls token events over: a new kind:7375 lists the ids of
//! the old ones it replaces in its `del` field (NIP-09 deletes are the
//! durable enforcement; `del` is the cross-reference this crate reads back).
//! Cold-start replay has no ordering guarantee, so `ingest_token_event` must
//! give the same answer regardless of which order events arrive in — this
//! reuses `WalletLedger`'s own confluence guard
//! (`WalletDerivedState::apply_token_tombstone` is safe to call BEFORE the
//! matching `apply_token_live`, see `ledger.rs`'s module docs) rather than
//! reinventing a parallel de-dup mechanism: every `del` entry is folded as a
//! `TokenDeleted` fact (and its proofs dropped from `state.proofs`) before
//! this event's own proofs are considered, and this event is skipped
//! entirely if the ledger already knows it as either live (already ingested
//! — a relay resend) or tombstoned (superseded by an earlier-processed `del`
//! that named it, however out of order).
//!
//! # Mint check-state on recovered proofs (issue #2977)
//!
//! The local `del`/dedup confluence above proves a recovered proof was not
//! superseded *by an event this account's relays carry* — but a proof can be
//! spent at the mint by another client/device whose rollover never reached
//! those relays (e.g. that client crashed after its swap but before
//! publishing the replacement kind:7375). Counting such a proof as spendable
//! shows a balance a subsequent send can't actually spend (it would fail
//! safely at the mint's swap — never a double-spend, see `send_worker.rs` —
//! but the displayed balance is wrong until then).
//!
//! So after folding a recovered token event's proofs into state,
//! [`reconcile_recovered_proofs`] runs a NUT-07 `check_state` pass over them
//! (batched per mint) and folds `WalletFact::MintProbed{Spent}` for any the
//! mint reports already-spent — the exact per-proof reconciliation mechanism
//! `send_worker.rs` uses post-swap, so an already-spent recovered proof drops
//! out of the ledger's spendable balance and out of the secret-bearing
//! inventory. The HTTP round-trip runs off the actor thread (D8); the pass is
//! best-effort/fail-open (a mint that errors or is unreachable leaves its
//! proofs counted — recovery must never zero a balance just because a mint is
//! transiently down; the swap-time safety net still holds).

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::build_nip44_decrypt_for_account;
use nmp_nip60::cashu::types::{Proof, ProofSpendState};
use nmp_nip60::cashu::MintClient;

use crate::journal::{
    DeleteCause, MintUrl, ProofAtom, ProofRef, ProofVerdict, Provenance, RelayRef, WalletEventId,
    WalletFact, WalletUnit,
};

use super::state::{canonicalize_mint_url, lock_state, CashuP2pkSecret, CashuWalletState};

/// Decoded kind:7375 plaintext — mirrors `nmp_nip60::token_event::TokenRecord`'s
/// wire shape, decoded here from a signer-transparent plaintext `String`
/// rather than that module's own `decode_token_event` (which takes a raw
/// `SecretKey` — D13, see module docs).
#[derive(serde::Deserialize)]
struct TokenEventPlaintext {
    mint: String,
    proofs: Vec<Proof>,
    #[serde(default)]
    del: Vec<String>,
}

/// One proof [`ingest_token_event`] freshly folded into spendable state, with
/// exactly the fields [`reconcile_recovered_proofs`] needs to run a NUT-07
/// check-state pass over it (#2977): its `mint` (canonical, so proofs batch
/// per real mint), its `secret` (the NUT-07 `Y = hash_to_curve(secret)` input),
/// and its `c` (the `ProofRef` key a resulting `MintProbed` fact — and the
/// secret-bearing inventory removal — key on). Carried out of the fold rather
/// than re-read from `state.proofs` so the pass probes *only* the proofs this
/// event actually added, never the whole inventory (and never touches proof
/// secrets it doesn't own).
pub(super) struct RecoveredProof {
    pub(super) mint: String,
    pub(super) secret: String,
    pub(super) c: String,
}

/// Load a decrypted kind:17375 plaintext into `state`. See module docs for
/// the never-clobber-an-existing-wallet invariant.
pub(super) fn ingest_wallet_config(
    state: &Mutex<CashuWalletState>,
    plaintext: &str,
) -> Result<(), String> {
    let pairs: Vec<Vec<String>> =
        serde_json::from_str(plaintext).map_err(|e| format!("wallet config decode: {e}"))?;

    let mut privkey_hex = None;
    let mut mints = Vec::new();
    for pair in pairs {
        if pair.len() < 2 {
            continue;
        }
        match pair[0].as_str() {
            "privkey" => privkey_hex = Some(pair[1].clone()),
            "mint" => mints.push(pair[1].clone()),
            _ => {}
        }
    }
    let privkey_hex = privkey_hex.ok_or_else(|| "wallet event missing privkey".to_string())?;
    if mints.is_empty() {
        return Err("wallet event has no mints".to_string());
    }
    let cashu_sk = <nostr::secp256k1::SecretKey as std::str::FromStr>::from_str(&privkey_hex)
        .map_err(|e| format!("wallet privkey parse: {e}"))?;
    let secp = nostr::secp256k1::Secp256k1::new();
    let cashu_pubkey_hex =
        hex::encode(nostr::secp256k1::PublicKey::from_secret_key(&secp, &cashu_sk).serialize());

    let mut s = lock_state(state);
    if s.created {
        // Already loaded — by an earlier ingest, a live `CreateCashuWallet`
        // this session, or a race between the two. Never overwrite: see
        // module docs.
        return Ok(());
    }
    s.mints = mints;
    s.cashu_pubkey_hex = Some(cashu_pubkey_hex);
    s.cashu_privkey = Some(CashuP2pkSecret(cashu_sk));
    s.created = true;
    Ok(())
}

/// Fold a decrypted kind:7375 plaintext for event `event_id` into `state`'s
/// ledger + secret-bearing proof store. `relay_hint` is whichever relay this
/// event was observed on (`KernelEvent::relay_provenance`'s first entry, or
/// empty when unknown) — carried into the ledger's `Provenance::Relay` fact,
/// never into anything logged. See module docs for the confluence
/// guarantee this relies on `WalletLedger` for.
///
/// Returns the proofs this call *freshly* folded (empty for a relay resend, a
/// superseded event, or an all-duplicate proof set) so the caller can drive
/// the NUT-07 check-state pass ([`reconcile_recovered_proofs`], #2977) over
/// exactly them — this function itself stays pure (no I/O, no port calls) and
/// directly unit-testable, per the module docs.
pub(super) fn ingest_token_event(
    state: &Mutex<CashuWalletState>,
    event_id: &str,
    plaintext: &str,
    relay_hint: &str,
) -> Result<Vec<RecoveredProof>, String> {
    let record: TokenEventPlaintext =
        serde_json::from_str(plaintext).map_err(|e| format!("token event decode: {e}"))?;
    let this_id = WalletEventId::new(event_id.to_string());
    // #2972 — canonicalize before this feeds the ledger fact (`add_proofs`
    // already canonicalizes internally, but the fact's `MintUrl` must be
    // built from the SAME canonical string so a recovered token's balance
    // fold lands under the same mint key every other deposit/send/redeem for
    // this real mint uses, rather than fragmenting balances by however this
    // account's own token event happened to spell it).
    let mint = canonicalize_mint_url(&record.mint);

    let mut s = lock_state(state);

    // Tombstone whatever this event's `del` supersedes FIRST — order-
    // independent (see module docs): a superseding event observed before
    // the token it supersedes still wins.
    for old_id in &record.del {
        s.ledger.apply(WalletFact::TokenDeleted {
            token_event: WalletEventId::new(old_id.clone()),
            cause: DeleteCause::Nip09Delete {
                by: this_id.clone(),
            },
        });
        s.remove_proofs_for_token_event(old_id);
    }

    // Already folded (relay resend) or already superseded by an
    // earlier-processed `del` naming this event: nothing further to do,
    // never double-counted.
    if s.ledger.state().is_token_live(&this_id) || s.ledger.state().is_token_tombstoned(&this_id) {
        return Ok(Vec::new());
    }

    // Belt-and-suspenders dedup by proof `C` beyond the per-event guard
    // above (#2965 requirement 3) — a proof already held under ANY other
    // token event is never re-added, even if some future producer emitted
    // it twice without a matching `del`.
    let known_cs: HashSet<&str> = s.proofs.iter().map(|p| p.proof.c.as_str()).collect();
    let fresh_proofs: Vec<Proof> = record
        .proofs
        .into_iter()
        .filter(|p| !known_cs.contains(p.c.as_str()))
        .collect();
    if fresh_proofs.is_empty() {
        return Ok(Vec::new());
    }

    let proof_atoms: Vec<ProofAtom> = fresh_proofs
        .iter()
        .map(|p| ProofAtom {
            proof: ProofRef::new(p.c.clone()),
            amount_msat: p.amount.saturating_mul(1000),
        })
        .collect();
    s.ledger.apply(WalletFact::TokenAdded {
        token_event: this_id,
        mint: MintUrl::new(mint.clone()),
        unit: WalletUnit::new("sat"),
        proofs: proof_atoms,
        via: Provenance::Relay(RelayRef::new(relay_hint)),
    });
    // Carry the freshly-folded proofs out for the check-state pass BEFORE
    // `add_proofs` consumes `fresh_proofs`. `mint` is already canonical (it is
    // the same value `add_proofs` stores), so a later probe batches under the
    // same key `state.proofs` holds these under.
    let recovered: Vec<RecoveredProof> = fresh_proofs
        .iter()
        .map(|p| RecoveredProof {
            mint: mint.clone(),
            secret: p.secret.clone(),
            c: p.c.clone(),
        })
        .collect();
    s.add_proofs(Some(event_id.to_string()), mint, fresh_proofs);
    Ok(recovered)
}

/// Run a NUT-07 check-state pass over `recovered` and fold the verdicts
/// (#2977). Batches one `MintClient::check_state` per mint and hands the
/// results to [`fold_check_state_verdicts`]. Best-effort / fail-open: a mint
/// that errors, is unreachable, or returns a mismatched count leaves its
/// proofs counted as spendable rather than zeroing a balance on a transient
/// mint outage — a real spend still fails safely at swap time against a
/// since-spent proof (see `send_worker.rs`). Runs off the actor thread (D8):
/// the caller spawns it on its own thread (see [`build_passive_ingest_command`]).
pub(super) fn reconcile_recovered_proofs(
    state: &Mutex<CashuWalletState>,
    recovered: Vec<RecoveredProof>,
) {
    let mut by_mint: BTreeMap<String, Vec<RecoveredProof>> = BTreeMap::new();
    for rp in recovered {
        by_mint.entry(rp.mint.clone()).or_default().push(rp);
    }
    for (mint, proofs) in by_mint {
        let client = MintClient::new(&mint);
        let secrets: Vec<String> = proofs.iter().map(|p| p.secret.clone()).collect();
        let Ok(states) = client.check_state(&secrets) else {
            // Fail-open — see this function's doc comment.
            continue;
        };
        // `parse_check_state_response` already guards length + `Y` ordering,
        // but never fold a mis-aligned reply: a state whose position no longer
        // matches its secret would mis-attribute one proof's verdict.
        if states.len() != proofs.len() {
            continue;
        }
        let verdicts: Vec<(String, ProofSpendState)> = proofs
            .iter()
            .zip(states.iter())
            .map(|(p, st)| (p.c.clone(), st.state))
            .collect();
        fold_check_state_verdicts(state, &verdicts);
    }
}

/// Fold NUT-07 check-state verdicts (#2977) keyed by proof `c`. For every
/// proof the mint reports `Spent`, apply `WalletFact::MintProbed{Spent}` (the
/// same absorbing per-proof verdict `send_worker.rs` folds post-swap, which
/// drops the proof from the ledger's spendable balance) AND remove it from the
/// secret-bearing `state.proofs` so no later send can select an already-spent
/// proof. `Unspent`/`Pending` verdicts are left entirely untouched — never
/// optimistically remove a still-spendable or in-flight proof. Pure fold-side
/// (no I/O), so it is directly unit-testable against a bare state.
pub(super) fn fold_check_state_verdicts(
    state: &Mutex<CashuWalletState>,
    verdicts: &[(String, ProofSpendState)],
) {
    let mut s = lock_state(state);
    for (c, spend_state) in verdicts {
        if !matches!(spend_state, ProofSpendState::Spent) {
            continue;
        }
        s.ledger.apply(WalletFact::MintProbed {
            proof: ProofRef::new(c.clone()),
            verdict: ProofVerdict::Spent,
        });
        s.proofs.retain(|p| p.proof.c != *c);
    }
}

/// Build the `ActorCommand` that decrypts a self-authored kind:17375/7375
/// event's content and ingests it — the passive cold-start-replay + live-tail
/// counterpart to `recover.rs`'s explicit `RecoverCashuWalletCommand`.
/// `mod.rs`'s `on_self_authored_wallet_event` returns this directly (no
/// wrapping `ProtocolCommand` needed: the continuation below only ever
/// mutates the captured `state`, so it needs no `CommandSender`).
///
/// Best-effort and silent on failure — never a `ShowErrorToken`/action-ledger
/// report: this path has no correlation id to report against, and a signer
/// that cannot NIP-44 would otherwise toast once per cold-start-replayed
/// event (bounded by `runtime.rs`'s `REPLAY_LIMIT`, i.e. potentially
/// hundreds) rather than once. `nmp.wallet.cashu.recover` is where a
/// definitive "this session's signer can't recover the wallet" failure is
/// meant to surface to the user (see `recover.rs`).
pub(super) fn build_passive_ingest_command(
    state: Arc<Mutex<CashuWalletState>>,
    account_pubkey: String,
    event_kind: u32,
    event_id: String,
    ciphertext: String,
    relay_hint: String,
) -> ActorCommand {
    build_nip44_decrypt_for_account(
        account_pubkey.clone(),
        ciphertext,
        Some(account_pubkey),
        move |outcome| {
            let Ok(plaintext) = outcome else {
                return;
            };
            if event_kind == nmp_nip60::kinds::KIND_NIP60_WALLET {
                let _ = ingest_wallet_config(&state, &plaintext);
                return;
            }
            let Ok(recovered) = ingest_token_event(&state, &event_id, &plaintext, &relay_hint)
            else {
                return;
            };
            if recovered.is_empty() {
                return;
            }
            // #2977 — verify the freshly-recovered proofs are still unspent at
            // the mint before leaving them counted as spendable balance. The
            // `check_state` HTTP round-trip must not run on the decrypt
            // continuation's thread (D8), so it moves to its own thread; the
            // fold it drives writes back through the shared `Arc<Mutex<..>>`
            // the same way every other off-actor-thread mint worker does.
            let state = Arc::clone(&state);
            std::thread::spawn(move || {
                reconcile_recovered_proofs(&state, recovered);
            });
        },
    )
}
