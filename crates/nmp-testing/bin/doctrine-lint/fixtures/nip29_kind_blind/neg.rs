//! Negative nip29_kind_blind fixture — must produce zero findings.
//!
//! Every `nmp.nip29.*` namespace here is an allowlisted lifecycle / admin /
//! envelope-routing verb, so nip29_kind_blind stays silent. The single
//! non-allowlisted verb carries a reason-bearing per-line opt-out, proving the
//! escape hatch suppresses the rule. No banned authoring constant appears.

pub struct PublishGroupEventAction;

impl PublishGroupEventAction {
    // The sole generic group-event write surface — always allowlisted.
    pub const NAMESPACE: &'static str = "nmp.nip29.publish_group_event";
}

pub struct CreatePublicGroupAction;

impl CreatePublicGroupAction {
    pub const NAMESPACE: &'static str = "nmp.nip29.create_public_group";
}

pub struct JoinGroupAction;

impl JoinGroupAction {
    pub const NAMESPACE: &'static str = "nmp.nip29.join";
}

// Per-line opt-out: a deliberately non-allowlisted verb suppressed with a
// reason-bearing escape hatch (the reason-required allow idiom).
pub struct ExperimentalAction;

impl ExperimentalAction {
    pub const NAMESPACE: &'static str = "nmp.nip29.experimental_probe"; // doctrine-allow: nip29_kind_blind — fixture: escape-hatch coverage
}
