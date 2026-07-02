//! Reusable active-account-change observation for UniFFI facades.
//!
//! A facade that owns app-specific, account-scoped sessions (for example 29er's
//! joined-groups view, pinned to the active account) needs to react when the
//! active account changes. The native runtime already exposes the seam —
//! [`NmpApp::register_identity_change_observer`] — and re-seeds
//! account-reactive feeds (`FeedScope::ActiveUserFollows`) in place through it,
//! so most facades need no per-session reopen at all.
//!
//! This module shares the Arc-sink + panic-containment plumbing that turns a
//! facade-local callback into that observer, so a facade does NOT:
//!
//! * hand-roll the `Arc`/`catch_unwind` wrapper, or
//! * capture a raw `*mut NmpApp` into the observer closure to act on the change.
//!
//! The observer receives only the new active-identity string and forwards it to
//! the facade's sink. Acting on the change (e.g. [`crate::reopen_feed`]
//! for a pinned feed) is done by the facade from one of its own methods,
//! where it already holds `&self.inner` — never from inside the observer with a
//! captured runtime pointer.

use std::sync::Arc;

use nmp_native_runtime::{IdentityChangeObserverId, NmpApp};

/// Register a facade-local sink as an active-account-change observer.
///
/// `on_change` receives the sink and the new active identity (`None` on
/// logout). Delivery is panic-contained: a panicking sink is swallowed so it
/// cannot poison the runtime's update-listener thread (matching
/// [`crate::set_lifecycle_callback`]).
///
/// Returns the [`IdentityChangeObserverId`] the facade passes to
/// [`unregister_account_change_sink`] at teardown.
pub fn register_account_change_sink<S, F>(
    app: &NmpApp,
    sink: Box<S>,
    on_change: F,
) -> IdentityChangeObserverId
where
    S: Send + Sync + 'static + ?Sized,
    F: Fn(&S, Option<String>) + Send + Sync + 'static,
{
    app.register_identity_change_observer(account_change_observer_from_sink(sink, on_change))
}

/// Revoke an observer registered by [`register_account_change_sink`].
/// Idempotent for unknown ids (D6).
pub fn unregister_account_change_sink(app: &NmpApp, id: IdentityChangeObserverId) {
    app.unregister_identity_change_observer(id);
}

/// Convert a facade-local account-change sink into the native-runtime observer
/// closure shape (`Fn(Option<String>)`), with panic containment.
///
/// Exposed for facades that register the observer through a different runtime
/// handle than `&NmpApp`; most facades use [`register_account_change_sink`].
#[must_use]
pub fn account_change_observer_from_sink<S, F>(
    sink: Box<S>,
    on_change: F,
) -> impl Fn(Option<String>) + Send + Sync + 'static
where
    S: Send + Sync + 'static + ?Sized,
    F: Fn(&S, Option<String>) + Send + Sync + 'static,
{
    let sink: Arc<S> = Arc::from(sink);
    let on_change = Arc::new(on_change);
    move |identity: Option<String>| {
        let sink = Arc::clone(&sink);
        let on_change = Arc::clone(&on_change);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            on_change(sink.as_ref(), identity);
        }));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[test]
    fn observer_delivers_identity_to_sink() {
        let seen: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let observer =
            account_change_observer_from_sink(Box::new(Arc::clone(&seen)), |seen, id| {
                seen.lock().unwrap().push(id);
            });

        observer(Some("b00b".to_string()));
        observer(None);

        assert_eq!(
            seen.lock().unwrap().as_slice(),
            &[Some("b00b".to_string()), None]
        );
    }

    #[test]
    fn panicking_sink_is_contained() {
        let observer = account_change_observer_from_sink(Box::new(()), |(), _id| {
            panic!("sink boom");
        });
        // Must not unwind into the caller (the runtime's listener thread).
        observer(Some("dead".to_string()));
    }

    #[test]
    fn register_then_unregister_no_panic() {
        let app = nmp_native_runtime::new_app();
        let id = register_account_change_sink(&app, Box::new(()), |(), _id| {});
        unregister_account_change_sink(&app, id);
        // Idempotent for unknown ids (D6).
        unregister_account_change_sink(&app, id);
    }
}
