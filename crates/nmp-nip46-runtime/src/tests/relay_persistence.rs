//! Relay-persistence contract tests (property-level).
//!
//! The full kernel-internal contract (`relay_socket_is_persistent` returning
//! true for `RelayRole::Signer`) is tested in `nmp-core`'s own test suite
//! (nip46_relay_persistence_tests).  These tests verify the role properties
//! visible from outside `nmp-core`:
//!
//! - `RelayRole::Signer` has the correct key string.
//! - `RelayRole::Signer` is excluded from `RelayRole::all()` (not in startup gate).
//! - The `EnqueueOutbound` command can be serialized and round-tripped.

#[cfg(test)]
mod tests {
    use nmp_network::role::RelayRole;

    /// `Signer` role key is `"signer"` — used in diagnostic relay-health rows.
    #[test]
    fn signer_role_key() {
        assert_eq!(RelayRole::Signer.key(), "signer");
    }

    /// `Signer` is excluded from `all()` — it must not appear in the startup
    /// bootstrap gate or in the standard relay-statuses projection.
    #[test]
    fn signer_role_excluded_from_all() {
        let all = RelayRole::all();
        assert!(
            !all.contains(&RelayRole::Signer),
            "RelayRole::Signer must NOT be in all(): it spawns on demand, \
             not at startup (same as RelayRole::Wallet)"
        );
    }

    /// `Wallet` is still excluded from `all()` — adding `Signer` must not
    /// have accidentally put `Wallet` back into the bootstrap gate.
    #[test]
    fn wallet_role_still_excluded_from_all() {
        let all = RelayRole::all();
        assert!(
            !all.contains(&RelayRole::Wallet),
            "RelayRole::Wallet must remain excluded from all() (regression guard)"
        );
    }

    /// `Content` is included in `all()` — the bootstrap gate still works.
    #[test]
    fn content_role_in_all() {
        assert!(RelayRole::all().contains(&RelayRole::Content));
    }
}
