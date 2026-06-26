//! Tests for NIP-46 bunker-broker wiring in the browser runtime (#2068).
//!
//! All tests are native-only: `nmp-signer-broker` is excluded from the wasm32
//! dependency graph. The wasm32 host-brokered path is covered by the existing
//! `signer.rs` `deliver_signer_response` tests (D4 channel + wake path).

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::sync::Arc;

    use nmp_signers::{LocalKeySigner, Signer};

    use crate::signer::nip46::PROVIDER_REG_CHANNEL_CAP;

    fn started_handle() -> crate::BrowserRuntimeHandle {
        crate::BrowserAppBuilder::new()
            .in_memory()
            .consume_all_builtin_projections()
            .without_initial_relays()
            .decide_providers(crate::BrowserRunConfig::default())
            .start()
    }

    fn make_local_signer() -> Arc<dyn Signer> {
        let s = LocalKeySigner::from_secret_hex(&"aa".repeat(32)).expect("valid secret");
        Arc::new(s) as Arc<dyn Signer>
    }

    /// D4 verification: a provider enqueued via the test seam is NOT visible to
    /// `signer_registry` until `pump()` is called. After `pump()`, the registry
    /// contains an entry for the signer's pubkey.
    #[test]
    fn nip46_provider_registration_via_channel_applied_on_pump() {
        let mut handle = started_handle();
        let signer = make_local_signer();
        let pubkey_hex = signer.pubkey().to_hex();

        // Registry must be empty before the registration is enqueued.
        assert!(
            handle.capability_envelope(&pubkey_hex).is_none(),
            "registry must be empty before enqueue"
        );

        // Enqueue a registration (as the broker event handler would do on the
        // OS thread after a successful NIP-46 handshake).
        handle.enqueue_nip46_provider_for_test(Arc::clone(&signer));

        // D4: the registry is NOT mutated by the enqueue; still empty.
        assert!(
            handle.capability_envelope(&pubkey_hex).is_none(),
            "registry must remain empty before pump() (D4 single-writer)"
        );

        // pump() drains the registration channel and applies it.
        handle.pump();

        // Now the registry must contain the provider.
        assert!(
            handle.capability_envelope(&pubkey_hex).is_some(),
            "registry must contain the provider after pump()"
        );
    }

    /// Channel capacity guard: the bounded channel silently drops the
    /// (PROVIDER_REG_CHANNEL_CAP + 1)-th enqueue rather than panicking (D6).
    #[test]
    fn nip46_provider_reg_drain_bounded() {
        use crate::signer::nip46::{provider_registration_channel, ProviderRegistration};

        let (tx, _rx) = provider_registration_channel();

        // Fill the channel to capacity.
        for _ in 0..PROVIDER_REG_CHANNEL_CAP {
            let signer = make_local_signer();
            let result = tx.try_send(ProviderRegistration { signer });
            assert!(result.is_ok(), "send within capacity must succeed");
        }

        // The (capacity + 1)-th send must fail (channel full), NOT panic.
        let signer = make_local_signer();
        let result = tx.try_send(ProviderRegistration { signer });
        assert!(
            result.is_err(),
            "send beyond PROVIDER_REG_CHANNEL_CAP must fail (bounded)"
        );
    }

    /// Smoke-test: `connect_nip46` and `cancel_nip46` are callable without panic.
    ///
    /// We cannot exercise a real NIP-46 handshake in a unit test (requires a
    /// live relay), but we can verify the methods exist, compile, and do not
    /// panic on a malformed URI or an idle cancel.
    #[test]
    fn connect_and_cancel_nip46_are_callable_without_panic() {
        let handle = started_handle();

        // A malformed URI is rejected inside the broker (handshake thread errors
        // out) — it must NOT panic here.
        handle.connect_nip46("not-a-valid-bunker-uri".to_string());

        // Cancel with no active session must be idempotent / no-op.
        handle.cancel_nip46();
    }
}
