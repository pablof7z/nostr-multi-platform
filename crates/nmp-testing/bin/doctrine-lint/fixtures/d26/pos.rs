// D26 positive fixture — ambient-authority references in protocol/command code
// that MUST fire (Workstream D item 7 / K2 + D6 lock-in). Each line below either
// names the broad `AppHost` super-trait (narrow protocol modules must take the
// specific registrar/capability trait) or reaches the raw `active_local_keys`
// signing-key accessor (protocol commands must sign via the signer-session port).

use crate::substrate::AppHost; // (1) importing the broad super-trait

// (2) `AppHost` as an `impl Trait` registration bound.
pub fn register(host: &impl AppHost) {
    host.register_ingest_parser(1, parser());
}

// (3) `AppHost` as a generic type-parameter bound.
pub fn wire<H: AppHost>(host: &H) {
    let _ = host;
}

// (4) A protocol command reaching the raw active-account signing keys.
pub fn sign_note(ctx: &ProtocolCommandContext) -> Option<Event> {
    let keys = ctx.active_local_keys()?;
    Some(mint(&keys))
}

// (5) Bareword `active_local_keys()` call (no receiver).
fn seal() -> Option<Keys> {
    active_local_keys()
}
