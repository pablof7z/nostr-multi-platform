//! Negative D9 fixture — must produce zero D9 findings.
//!
//! Every action namespace declared here uses the `nmp.<…>.<verb>` shape, so
//! D9 stays silent. The substrings used here deliberately avoid D0's banned
//! tokens (`group_id`, `nip29`, `pin_to`, …) so D0 also stays silent when
//! the staged fixture is scanned from outside the exempt path tree.

pub struct PublishAction;

impl PublishAction {
    pub const NAMESPACE: &'static str = "nmp.publish";
}

pub struct SendDmAction;

impl SendDmAction {
    pub const NAMESPACE: &'static str = "nmp.nip17.send";
}

pub struct ZapAction;

impl ZapAction {
    pub const NAMESPACE: &'static str = "nmp.zap";
}

pub struct KeyringCapability;

impl KeyringCapability {
    pub const NAMESPACE: &'static str = "nmp.keyring.capability";
}

// Per-line opt-out: the `// doctrine-allow: D9` escape hatch suppresses the
// rule for a single legitimately-non-`nmp.`-prefixed literal.
pub struct ExternalNamespaceFallback;

impl ExternalNamespaceFallback {
    pub const NAMESPACE: &'static str = "external.fallback"; // doctrine-allow: D9 — third-party-host bridge fixture
}

