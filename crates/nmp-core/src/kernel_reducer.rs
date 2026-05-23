//! Public pure reducer over [`KernelAction`] → [`KernelUpdate`].
//!
//! `nmp-codegen` projects per-app FFI crates that own an `AppAction` /
//! `AppUpdate` pair around [`KernelAction`] / [`KernelUpdate`]. The generated
//! `FfiApp::dispatch` needs to reduce the kernel arm to an update — but the
//! [`crate::kernel_action::dispatch_kernel_action`] reducer (also used by the
//! actor loop) is `pub(crate)` and takes a private `&mut Kernel`, neither
//! reachable from a downstream crate.
//!
//! [`KernelReducer`] closes that seam: it owns an encapsulated [`Kernel`] and
//! exposes a single public method — [`KernelReducer::reduce`] — that delegates
//! to the same hand-written reducer the actor uses. Behaviour is byte-for-byte
//! identical with the actor path for every [`KernelAction`] variant,
//! including [`KernelAction::OpenUri`] (which registers a subscription
//! interest through the kernel's single-writer registry).
//!
//! Doctrine:
//! - **D0** — the public surface deals only in app-noun-free primitives.
//! - **D6** — total function: never panics, never unwinds across FFI.
//!   Failures funnel into [`KernelUpdate::UriRejected`].
//! - **D8** — runs once per *action*, not per ingested event.
//!
//! This is the NMP-145 follow-up: T-NMP-145-FF.

use crate::kernel_action::dispatch_kernel_action;
use crate::app::{KernelAction, KernelUpdate};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// Encapsulated kernel + public pure reducer.
///
/// Owns the [`Kernel`] privately so codegen-driven `FfiApp`s can reduce
/// [`KernelAction`] values to [`KernelUpdate`] values without depending on
/// crate-internal types.
pub struct KernelReducer {
    kernel: Kernel,
}

impl KernelReducer {
    /// Construct a fresh reducer with the default visible-limit. Equivalent
    /// to what the actor loop uses at startup.
    #[must_use] 
    pub fn new() -> Self {
        Self {
            kernel: Kernel::new(DEFAULT_VISIBLE_LIMIT),
        }
    }

    /// Reduce one [`KernelAction`] against the encapsulated kernel, returning
    /// the [`KernelUpdate`] the host app should observe.
    ///
    /// Total and panic-free (D6): the only fallible action (`OpenUri`)
    /// funnels its typed error into [`KernelUpdate::UriRejected`].
    pub fn reduce(&mut self, action: KernelAction) -> KernelUpdate {
        dispatch_kernel_action(&mut self.kernel, action)
    }
}

impl Default for KernelReducer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::VIEW_PROFILE;
    use crate::nip19::encode_npub;

    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

    #[test]
    fn reduce_open_uri_npub_routes_to_profile_view() {
        let mut r = KernelReducer::new();
        let npub = encode_npub(PK).unwrap();
        let update = r.reduce(KernelAction::OpenUri {
            uri: format!("nostr:{npub}"),
        });
        assert_eq!(
            update,
            KernelUpdate::ViewOpened {
                namespace: VIEW_PROFILE.into(),
                key: PK.into(),
            }
        );
    }

    #[test]
    fn reduce_start_echoes_started() {
        let mut r = KernelReducer::new();
        assert_eq!(r.reduce(KernelAction::Start), KernelUpdate::Started { rev: 0 });
    }

    #[test]
    fn reduce_garbage_uri_is_rejected_not_a_panic() {
        let mut r = KernelReducer::new();
        let update = r.reduce(KernelAction::OpenUri {
            uri: "not-a-nostr-thing".into(),
        });
        assert!(matches!(
            update,
            KernelUpdate::UriRejected { reason, .. } if reason.contains("unparseable")
        ));
    }
}
