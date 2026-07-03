/// Protocol-owned validator for external event-reference identifiers.
///
/// `nmp-core` owns the `i:` projection-key prefix and byte hygiene, but not the
/// NIP-specific scheme grammar behind the stripped identifier.
pub trait ExternalIdValidator: Send + Sync {
    fn is_valid_external_id(&self, external_id: &str) -> bool;
}
