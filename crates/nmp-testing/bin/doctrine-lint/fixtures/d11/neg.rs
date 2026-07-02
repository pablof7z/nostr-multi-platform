//! Negative D11 fixture — must produce ZERO findings of any rule.
//!
//! Exercises D11 exemptions:
//!   1. A plain helper, not decorated by the UniFFI export attribute, builds
//!      a banned variant — D11 only fires inside an exported scope, so this
//!      is exempt.
//!   2. A regular (non-exported) `impl` block is out of scope — D11 is the
//!      door for the UniFFI publish surface, not every impl in the workspace.
//!   3. A trailing-comment `// doctrine-allow: D11` opts out a specific
//!      body line (the standard doctrine escape hatch).
//!
//! Care: the fixture text is ALSO scanned for D6/D7/D8 — no `.unwrap()` /
//! `todo!()` / hot-path allocations / sleeps may appear, and none of these
//! doc-comment lines may literally spell the export attribute token (the
//! tracker's attribute detection is not comment-aware, matching the
//! pre-existing precedent in the deleted `extern "C" fn nmp_app_*` tracker
//! this rule replaces) — or the negative assertion (zero findings of any
//! rule) breaks.

// (1) Non-exported helper — D11 must not fire. This is exactly the
// `kernel::action_registry` shape (the GOOD path: dispatch_action's
// executor builds a `PublishSignedEvent`).
pub fn route_publish_action() {
    let _ = ActorCommand::Publish(PublishCommand::SignedEvent {
        raw: r,
        relays: v,
        correlation_id: c,
    });
}

// (2) A plain, non-exported impl block — out of D11 scope.
impl NotExported {
    pub fn hypothetical(&self) {
        let _ = ActorCommand::Publish(PublishCommand::SignedEvent {
            raw: r,
            relays: v,
            correlation_id: c,
        });
    }
}

// (3) Per-line escape hatch — explicit opt-out on the offending body line.
#[uniffi::export]
impl ExemptViaAllow {
    pub fn exempt(&self) {
        let cmd = PublishCommand::SignedEvent { event }; // doctrine-allow: D11 - fixture exemption proof
        let _ = ActorCommand::Publish(cmd);
    }
}
