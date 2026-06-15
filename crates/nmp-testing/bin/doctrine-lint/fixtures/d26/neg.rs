// D26 negative fixture — none of these lines may fire. They exercise every
// accepted shape: narrow capability/registrar trait bounds (the D6 goal), the
// signer-session port (the signing one-door), boundary-anchored near-misses, and
// the common "this crate does NOT take AppHost" doc-comment annotation.

// Accepted: a doc comment naming AppHost — the canonical narrow-consumer
// annotation. `AppHost` here is in a comment, never a bound.
//! This crate registers through narrow traits, never the broad `AppHost`.

use crate::substrate::{HostCapabilities, IngestParserRegistrar, RelayConnectedHookRegistrar};

// Accepted: narrow registrar bound — the D6 goal pattern.
pub fn register(host: &impl IngestParserRegistrar) {
    host.register_ingest_parser(1, parser());
}

// Accepted: another narrow trait bound (the relay-connected hook seam).
pub fn wire<H: RelayConnectedHookRegistrar>(host: &H) {
    let _ = host;
}

// Accepted: a capability accessor that is NOT the raw-keys reach.
pub fn pubkey(host: &impl HostCapabilities) -> ActiveAccountSlot {
    host.active_pubkey()
}

// Accepted: signing through the signer-session port, pinned to the
// backend-transparent active-account pubkey — the correct one-door.
pub fn sign_note(ctx: &ProtocolCommandContext) -> Option<()> {
    let signer_pubkey = ctx.active_account_pubkey()?;
    ctx.sign_event_for_account(builder(), Some(signer_pubkey));
    Some(())
}

// Accepted: boundary-anchored near-misses — longer identifiers that merely
// contain the banned tokens never fire.
pub struct AppHostImpl;
fn slot() -> ActiveLocalKeysSlot {
    new_active_local_keys_slot()
}
fn cache() {
    let _ = self_active_local_keys_cache();
}
