//! Restore a fully-handshaken NIP-46 session for steady-state signing.
//!
//! For sessions restored from a persistence payload (`SignerPayload::Nip46`)
//! the handshake has already completed. The runtime reuses the same
//! `local_keys` + `remote_pubkey` that were in the original session; no
//! re-handshake is needed.
//!
//! The resulting [`SessionState`] is in the `Done` phase so the reducer
//! ignores all further relay inputs. Steady-state inbound responses are decoded
//! at the runtime layer via [`crate::rpc::decode_inbound_response`] rather than
//! through the reducer — the interceptor's steady-state path handles any `EVENT`
//! on the session's `sub_id` regardless of phase.
//!
//! The returned [`Effect::Subscribe`] vector carries one `REQ` frame per relay
//! in `relay_urls` so the caller can open persistent subscription(s) and
//! register reconnect preambles immediately.

use nostr::Keys;

use crate::effect::Effect;
use crate::reducer::{Phase, SessionState};
use crate::rpc::build_req_frame;

/// Create a terminal [`SessionState`] + initial `Subscribe` effects for a
/// restored NIP-46 session.
///
/// The state is in the `Done` phase — all further reducer inputs are silently
/// ignored (D6). Steady-state RPC responses are decoded at the runtime layer
/// via [`crate::decode_inbound_response`] rather than through the reducer.
///
/// Returns `(state, effects)` where `effects` contains one
/// [`Effect::Subscribe`] per entry in `relay_urls`. The caller must:
/// 1. Process each `Effect::Subscribe` (send REQ, register persistent sub).
/// 2. Install the signer via `CommandSender::add_signer` using the persisted
///    remote user pubkey.
///
/// No [`Effect::SendFrame`] is emitted — a restored session never re-runs the
/// `connect` / `get_public_key` handshake.
#[must_use]
pub fn start_restore(
    sub_id: &str,
    local_keys: Keys,
    relay_urls: &[String],
    now_secs: u64,
) -> (SessionState, Vec<Effect>) {
    let primary_relay = relay_urls.first().cloned().unwrap_or_default();
    let state = SessionState::new(
        Phase::Done,
        local_keys.clone(),
        primary_relay,
        sub_id.to_string(),
        None,
        0,
        0, // no deadline for Done sessions
    );
    let pubkey_hex = local_keys.public_key().to_hex();
    let effects = relay_urls
        .iter()
        .map(|relay_url| {
            let frame = build_req_frame(sub_id, &pubkey_hex, now_secs);
            Effect::Subscribe { relay_url: relay_url.clone(), frame }
        })
        .collect();
    (state, effects)
}
