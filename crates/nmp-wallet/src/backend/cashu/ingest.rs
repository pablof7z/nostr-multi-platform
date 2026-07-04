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
//! # Mint check-state on recovered proofs (#2977)
//!
//! `ingest_token_event` itself only ever applies the local `del`/dedup
//! confluence above — it never asks the mint whether a proof is still
//! unspent. That reconciliation is [`super::check_state::run_check_state_pass`],
//! kicked off by this module's `build_passive_ingest_command` continuation
//! (below, whenever fresh proofs were actually folded) and by
//! `recover.rs`'s `RecoverCashuWalletCommand` (which additionally defers its
//! own `RecordActionSuccess` until that pass completes, so a caller polling
//! balance right after `nmp.wallet.cashu.recover` returns sees the
//! mint-reconciled, UNSPENT-only figure). See that module's docs for the
//! fail-safe (never drop a proof the mint didn't affirmatively report
//! spent).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::build_nip44_decrypt_for_account;
use nmp_nip60::cashu::types::Proof;

use crate::journal::{
    DeleteCause, MintUrl, ProofAtom, ProofRef, Provenance, RelayRef, WalletEventId, WalletFact,
    WalletUnit,
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
/// Returns whether this call actually folded fresh proofs into `state` (as
/// opposed to a no-op: a relay resend, a `del`-superseded event, or an
/// event whose proofs were already held under some other token event) — the
/// caller (`build_passive_ingest_command`, below) uses this to decide
/// whether a mint check-state pass (#2977) is worth kicking off at all.
pub(super) fn ingest_token_event(
    state: &Mutex<CashuWalletState>,
    event_id: &str,
    plaintext: &str,
    relay_hint: &str,
) -> Result<bool, String> {
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
        return Ok(false);
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
        return Ok(false);
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
    s.add_proofs(Some(event_id.to_string()), mint, fresh_proofs);
    Ok(true)
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
            let Ok(folded_fresh_proofs) =
                ingest_token_event(&state, &event_id, &plaintext, &relay_hint)
            else {
                return;
            };
            if folded_fresh_proofs {
                // #2977 — this event actually added proofs (not a relay
                // resend / del-superseded no-op): reconcile them against
                // their mint before they can be read as final balance. See
                // module docs ("Mint check-state on recovered proofs").
                // Silent and off the actor thread, same posture as the
                // rest of this passive path — no correlation id to report
                // against here. Debounced (`spawn_debounced`, not a raw
                // `run_check_state_pass` thread per event): cold-start
                // replay can fold many kind:7375 events in a tight,
                // unordered burst, and this collapses that burst into at
                // most two outstanding mint round-trips regardless of size.
                super::check_state::spawn_debounced(Arc::clone(&state));
            }
        },
    )
}
