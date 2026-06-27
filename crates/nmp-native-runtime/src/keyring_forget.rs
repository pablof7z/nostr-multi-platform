//! `NmpApp::remove_account_forgetting_keyring` — the D6 fail-loud account
//! removal seam.
//!
//! Split out of `lib.rs` so the checked-result body does not push that file
//! past its LOC ceiling. The behaviour: forget the app-scoped keyring secret
//! first, then remove the account ONLY when the secret is confirmed gone.

use super::NmpApp;
use nmp_core::substrate::KeyringStatus;

impl NmpApp {
    /// Forget the app-scoped local secret and remove the identity through the
    /// actor-owned reducer.
    ///
    /// D6 fail-loud: the keyring forget result is **checked**, not discarded.
    /// If the OS keychain reports `Error` the nsec is still resident — removing
    /// the account anyway would orphan the secret in the keychain (a
    /// security/privacy residue), so we surface a toast and keep the account so
    /// the host can retry. The account is removed only when the secret is
    /// confirmed gone (`Ok`) or was never there (`NotFound`).
    ///
    /// Returns the keyring status so callers (e.g. `nmp-marmot::identity`) can
    /// react to a failed forget instead of assuming success.
    pub fn remove_account_forgetting_keyring(
        &self,
        account_id: &str,
        identity_id: String,
    ) -> KeyringStatus {
        let req = nmp_core::substrate::KeyringIdentityWiring::forget_secret(
            "nmp.identity.forget",
            account_id,
        );
        let envelope = self.dispatch_capability(&req);
        let status = nmp_core::substrate::KeyringIdentityWiring::decode_result(&envelope).status;
        match status {
            KeyringStatus::Ok | KeyringStatus::NotFound => {
                self.remove_account(identity_id);
            }
            KeyringStatus::Error => {
                // Do NOT remove the account: the secret is still in the
                // keychain. Surface the failure so the host can retry rather
                // than silently orphaning the nsec.
                self.show_toast(format!(
                    "could not forget the stored key for this account ({account_id}); \
                     account kept to avoid leaving the key behind"
                ));
            }
        }
        status
    }
}
