//! The per-envelope §D6 gift-UNWRAP port chain: outer decrypt → seal decrypt →
//! store.
//!
//! The receive-side twin of [`crate::dm_send`]'s `chain.rs`. Each step enqueues
//! the next via a cloned [`CommandSender`]; the chain owns all data for one
//! envelope and self-drives. Extracted from `inbox.rs` to keep that file within
//! its LOC ceiling and to localise the async decrypt machinery.
//!
//! # Two sequential NIP-44 decrypts (ADR-0050 §D6)
//!
//! Unsealing ONE kind:1059 envelope needs two sequential decrypts, each routed
//! through `ActorCommand::Nip44DecryptForAccount`:
//!
//! 1. **outer** — decrypt `gift_wrap.content` against the ephemeral wrap pubkey
//!    (`gift_wrap.pubkey`) → the kind:13 seal JSON.
//! 2. **inner** — decrypt `seal.content` against the seal author (`seal.pubkey`)
//!    → the kind:14 rumor JSON.
//!
//! The pure parse/verify halves live in `nmp_nip59` ([`parse_outer_for_decrypt`]
//! / [`parse_seal_for_decrypt`] / [`parse_rumor`]); this chain only performs the
//! key-bearing decrypts (through the port — the inbox holds no `Keys`) and the
//! plumbing between them.
//!
//! # Backend transparency + account pinning (§D6)
//!
//! `signer_pubkey` is pinned to `Some(active_hex)` on BOTH steps — the chain
//! never re-resolves "active", so a mid-flight account switch cannot decrypt the
//! seal with a different key than the one the outer step used. A **local**
//! account resolves each decrypt `Ready` and the continuation runs INLINE on the
//! actor thread (the whole chain completes across consecutive dispatches); a
//! **bunker** account parks each decrypt and the continuation runs from the
//! mailbox-completion drain. This code cannot tell which (V-78).
//!
//! # D doctrine
//!
//! * **D6** — every failure (envelope not for us, decrypt error, non-kind:14,
//!   seal-author spoof, stale epoch) is a silent discard. No toast: an inbound
//!   envelope we cannot decrypt is simply not ours (unlike a SEND failure, which
//!   the user initiated and must hear about).
//! * **D8** — each continuation only enqueues the next port command or performs
//!   one bounded store insert; no blocking, no I/O.
//! * **D13** — no raw `nostr::Keys` cross this module; only ciphertext/plaintext
//!   and a pinned hex pubkey are observed.

use std::sync::Arc;

use nmp_core::substrate::build_nip44_decrypt_for_account;
use nmp_core::CommandSender;
use nostr::Event;

use super::store::{first_p_tag, first_reply_e_tag, source_relays_from, InboxStore};
use super::DmMessage;

/// Launch the outer decrypt for one accepted kind:1059 envelope. Returns `true`
/// when the chain was launched (the outer envelope parsed and a decrypt command
/// was enqueued); `false` for the pre-launch no-op (a malformed/non-1059
/// envelope — the outer parse half rejected it).
///
/// `signer_hex` is the §D6-pinned active account; `generation` is the §D6 epoch
/// captured at envelope arrival; `source_relay_url` is the live-relay provenance
/// (always `None` on the `IngestParser` seam).
pub(super) fn launch_unwrap(
    tx: CommandSender,
    store: Arc<InboxStore>,
    signer_hex: String,
    generation: u64,
    event: Event,
    source_relay_url: Option<String>,
) -> bool {
    // The active account this envelope is being unwrapped for — used for the
    // cheap `#p == signer` defense-in-depth in `parse_outer_for_decrypt` (issue
    // #1265). A malformed pinned pubkey is a silent no-op (D6).
    let Ok(recipient) = nostr::PublicKey::from_hex(&signer_hex) else {
        return false;
    };
    // Pure half 1 — validate kind:1059, confirm it addresses us, and extract
    // (outer_ciphertext, ephemeral peer). A non-gift-wrap or a wrap not addressed
    // to the active account is a silent no-op (D6); nothing is enqueued.
    let Ok((outer_ciphertext, ephemeral_peer)) =
        nmp_nip59::parse_outer_for_decrypt(&event, &recipient)
    else {
        return false;
    };

    let tx_for_inner = tx.clone();
    let cmd = build_nip44_decrypt_for_account(
        ephemeral_peer.to_hex(),
        outer_ciphertext,
        Some(signer_hex.clone()),
        move |outcome| {
            // Runs on the actor thread (inline local / drain bunker). D8: only
            // enqueues the next port step or discards.
            let Ok(seal_plaintext) = outcome else {
                // Decrypt failed → the envelope was not addressed to us (or is
                // another protocol's kind:1059). Silent discard (D6).
                return;
            };
            decrypt_seal(
                tx_for_inner,
                store,
                signer_hex,
                generation,
                seal_plaintext,
                source_relay_url,
            );
        },
    );
    tx.send(cmd).is_ok()
}

/// Step 2 — parse + verify the decrypted kind:13 seal, then enqueue the INNER
/// `Nip44DecryptForAccount` against the seal author. Its continuation runs the
/// terminal store insert.
fn decrypt_seal(
    tx: CommandSender,
    store: Arc<InboxStore>,
    signer_hex: String,
    generation: u64,
    seal_plaintext: String,
    source_relay_url: Option<String>,
) {
    // Pure half 2 — parse + signature-verify the seal, extract (seal, inner
    // ciphertext, seal author). A malformed/forged seal is a silent discard (D6).
    let Ok((seal, inner_ciphertext, seal_author)) =
        nmp_nip59::parse_seal_for_decrypt(&seal_plaintext)
    else {
        return;
    };

    let cmd = build_nip44_decrypt_for_account(
        seal_author.to_hex(),
        inner_ciphertext,
        Some(signer_hex.clone()),
        move |outcome| {
            let Ok(rumor_plaintext) = outcome else {
                return; // inner decrypt failed → discard (D6).
            };
            // Pure half 3 — parse the rumor + enforce author-matches-seal
            // (anti-spoof). A mismatch/malformed rumor is a silent discard (D6).
            let Ok(gift) = nmp_nip59::parse_rumor(&seal, &rumor_plaintext) else {
                return;
            };
            store_rumor(&store, generation, &signer_hex, &gift, source_relay_url.as_deref());
        },
    );
    let _ = tx.send(cmd);
}

/// Terminal step — apply the kind:14 gate, classify peer/direction, and insert
/// into the store under the captured §D6 epoch.
fn store_rumor(
    store: &InboxStore,
    generation: u64,
    signer_hex: &str,
    gift: &nmp_nip59::UnwrappedGift,
    source_relay_url: Option<&str>,
) {
    // Only kind:14 chat-message rumors belong in the DM inbox. Anything else
    // that happens to unwrap is discarded (D6).
    if gift.rumor.kind.as_u16() != nmp_kinds::KIND_CHAT_MESSAGE as u16 {
        return;
    }

    let sender_pubkey = gift.sender.to_hex();
    // The rumor id may be `None` if the sender did not pre-compute it;
    // `UnsignedEvent::id()` derives the canonical NIP-01 id deterministically
    // (and memoises it) so the inbox always has a stable dedupe key.
    let mut rumor = gift.rumor.clone();
    let message_id = rumor.id().to_hex();

    // The conversation peer is the OTHER party. A self-copy (sender == local)
    // files under the `p`-tag recipient; the received copy files under the
    // sender.
    let peer_pubkey = if sender_pubkey == signer_hex {
        match first_p_tag(&rumor) {
            Some(p) => p,
            None => return, // a self-copy with no `p` tag is malformed (D6).
        }
    } else {
        sender_pubkey.clone()
    };

    // Pre-classify outgoing vs incoming (thin-shell rule — the kind:13 seal
    // already authenticated `sender_pubkey`; the shell must not re-derive it).
    let is_outgoing = sender_pubkey == signer_hex;
    let message = DmMessage {
        id: message_id.clone(),
        sender_pubkey,
        content: rumor.content.clone(),
        // D7: the rumor's `created_at` is the sender's real send time.
        created_at: rumor.created_at.as_secs(),
        reply_to: first_reply_e_tag(&rumor),
        is_outgoing,
        source_relays: source_relays_from(source_relay_url),
    };

    // §D6 epoch-guarded idempotent insert (stale epoch / poisoned mutex →
    // silent no-op).
    store.insert(generation, message_id, peer_pubkey, message, source_relay_url);
}
