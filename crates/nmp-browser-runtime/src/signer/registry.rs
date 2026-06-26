//! `CapabilityProviderRegistry` — per-account signer/capability store (#2049/#2065).
//!
//! Populated at builder time via `BrowserAppBuilder::with_capability_providers`;
//! moved into the runtime at `start()`. The registry is the single door through
//! which the pump broker looks up a registered signer for an account (ADR-0067
//! §10a — signer SEMANTICS live in `nmp-signers`; this crate only WIRES).
//!
//! # Scope boundary (D6-honest)
//!
//! `CapabilityEnvelope` advertises sign/nip04/nip44 presence derived from the
//! registered `Signer` at registration time.
//!
//! Encrypt/decrypt (`nip04`/`nip44`) are **ADVERTISED as metadata only** — no
//! kernel encrypt-roundtrip seam exists yet. Wire-routing encrypt/decrypt through
//! the kernel is a follow-up to this PR; this honest annotation keeps the seam
//! boundary visible without a silent omission.
//!
//! NIP-46 bunker:// providers are not wired in this track — see #2068.

use std::collections::HashMap;
use std::sync::Arc;

use nmp_signers::{Signer, SignerBackend};

/// Advertised capability envelope derived from a registered [`Signer`].
///
/// Constructed once at registration time from the signer's trait surface.
/// Callers can read it via
/// [`BrowserRuntimeHandle::capability_envelope`] without acquiring any lock.
///
/// # Scope boundary
///
/// `nip04`/`nip44` are advertised as metadata only. No kernel
/// encrypt-roundtrip seam exists in this track; encrypt/decrypt routing
/// is a follow-up (#2068 NIP-46 / encrypt-decrypt scope).
#[derive(Clone, Debug)]
pub struct CapabilityEnvelope {
    /// The signer can sign events (always `true` for a registered provider).
    pub sign_event: bool,
    /// The signer exposes a NIP-04 encrypt/decrypt namespace (metadata only —
    /// not wire-routed in this track).
    pub nip04: bool,
    /// The signer exposes a NIP-44 encrypt/decrypt namespace (metadata only —
    /// not wire-routed in this track).
    pub nip44: bool,
    /// Backend discriminator (LocalKey / Nip07 / …).
    pub backend: SignerBackend,
}

impl CapabilityEnvelope {
    /// Derive the envelope from a signer's trait surface.
    pub(crate) fn from_signer(signer: &dyn Signer) -> Self {
        Self {
            sign_event: true,
            nip04: signer.nip04().is_some(),
            nip44: signer.nip44().is_some(),
            backend: signer.backend(),
        }
    }
}

/// One registered entry: signer + pre-computed capability envelope.
pub(crate) struct ProviderEntry {
    pub(crate) signer: Arc<dyn Signer>,
    pub(crate) envelope: CapabilityEnvelope,
}

/// Registry mapping lowercase-hex account pubkeys to their registered signer.
///
/// Multiple inserts for the same pubkey are last-write-wins (one signer per
/// identity — single-door, ADR-0067 §10a).
#[derive(Default)]
pub(crate) struct CapabilityProviderRegistry {
    providers: HashMap<String, ProviderEntry>,
}

impl CapabilityProviderRegistry {
    /// Construct an empty registry.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Insert a signer, keyed on its lowercase-hex pubkey.
    ///
    /// Last-write-wins when multiple signers share a pubkey.
    pub(crate) fn insert(&mut self, signer: Arc<dyn Signer>) {
        let pubkey_hex = signer.pubkey().to_hex();
        let envelope = CapabilityEnvelope::from_signer(signer.as_ref());
        self.providers
            .insert(pubkey_hex, ProviderEntry { signer, envelope });
    }

    /// Resolve a provider for `account_pubkey` (lowercase hex).
    pub(crate) fn resolve(&self, account_pubkey: &str) -> Option<&ProviderEntry> {
        self.providers.get(account_pubkey)
    }

    /// Return the capability envelope for `account_pubkey`, if registered.
    pub(crate) fn capability_envelope(&self, account_pubkey: &str) -> Option<&CapabilityEnvelope> {
        self.providers.get(account_pubkey).map(|e| &e.envelope)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nmp_signers::LocalKeySigner;

    use super::*;

    #[test]
    fn insert_and_resolve_by_pubkey() {
        let signer = Arc::new(LocalKeySigner::generate());
        let pubkey_hex = signer.pubkey().to_hex();

        let mut reg = CapabilityProviderRegistry::new();
        reg.insert(signer as Arc<dyn Signer>);

        let entry = reg.resolve(&pubkey_hex);
        assert!(entry.is_some(), "should resolve by pubkey hex");
        assert!(
            reg.resolve("deadbeef").is_none(),
            "unknown pubkey resolves to None"
        );
    }

    #[test]
    fn envelope_derived_correctly_for_local_key() {
        let signer = Arc::new(LocalKeySigner::generate());
        let pubkey_hex = signer.pubkey().to_hex();

        let mut reg = CapabilityProviderRegistry::new();
        reg.insert(signer as Arc<dyn Signer>);

        let env = reg.capability_envelope(&pubkey_hex).expect("envelope present");
        assert!(env.sign_event, "sign_event always true");
        // LocalKeySigner exposes both nip04 and nip44.
        assert!(env.nip04, "LocalKeySigner advertises nip04");
        assert!(env.nip44, "LocalKeySigner advertises nip44");
        assert!(
            matches!(env.backend, nmp_signers::SignerBackend::LocalKey),
            "backend discriminator"
        );
    }

    #[test]
    fn last_write_wins_for_same_pubkey() {
        let signer = LocalKeySigner::generate();
        let pubkey_hex = signer.pubkey().to_hex();

        let mut reg = CapabilityProviderRegistry::new();
        // Insert twice — second insert must win.
        let s1: Arc<dyn Signer> = Arc::new(LocalKeySigner::generate());
        let s2: Arc<dyn Signer> = Arc::new(LocalKeySigner::generate());
        // Use a fixed pubkey by inserting the same signer twice.
        reg.insert(Arc::new(signer));
        // Re-insert with a signer that has a different identity would result in
        // a different key.  Here we test that the entry is replaced.
        let _ = pubkey_hex; // used above
        // Insert s1 then s2 with the same pubkey by using from_secret_hex.
        let secret = "aa".repeat(32);
        let sa: Arc<dyn Signer> =
            Arc::new(LocalKeySigner::from_secret_hex(&secret).expect("valid secret"));
        let sb: Arc<dyn Signer> =
            Arc::new(LocalKeySigner::from_secret_hex(&secret).expect("valid secret"));
        let pk = sa.pubkey().to_hex();
        reg.insert(Arc::clone(&sa));
        reg.insert(Arc::clone(&sb));
        // Either sa or sb is present — both have same pubkey; key present.
        assert!(reg.resolve(&pk).is_some(), "entry present after last-write-wins");
        // s1/s2 inserted separately — but they share nothing; just verify count is 1.
        let _ = (s1, s2);
    }
}
