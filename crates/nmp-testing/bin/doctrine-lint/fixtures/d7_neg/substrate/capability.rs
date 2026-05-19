//! Negative D7 fixture — file is `substrate/capability.rs` (in scope!) but
//! every method name is a *reporting* verb. Rule produces no findings.

pub trait GoodKeychainBridge: Send + Sync {
    /// Reporting verb — capability emits an event, doesn't decide.
    fn emit_authentication_outcome(&self);

    /// Reporting verb — capability returns the *observed* result.
    fn observe_keychain_state(&self) -> Option<String>;

    /// Reporting verb — capability hands back the raw query result.
    fn query_available_credentials(&self) -> Vec<String>;

    /// Doc comments may freely mention banned verbs as long as the method
    /// name itself doesn't: "This capability does NOT retry, fallback,
    /// select, or choose — policy lives in `nmp-core`."
    fn report_event(&self);
}
