//! Chirp relay-bootstrap seeding over the C-ABI.
//!
//! Seeding policy lives in Rust, not in the Swift/Kotlin shell (D7 /
//! thin-shell principle). This is the iOS analogue of the Android
//! `nmp-chirp-android-ffi::relay_seeding` glue: both wrap
//! [`nmp_chirp_config::chirp_default_relay_bootstrap`] so the relay default set
//! has ONE source of truth (`apps/chirp/crates/nmp-chirp-config`). Before this module
//! existed the iOS shell (`RelaySeeding.swift`) hardcoded its own relay URLs,
//! which had already drifted from the canonical config's current
//! `wss://relay.primal.net` content lane.
//!
//! Two entry points, mirroring Android:
//!
//! * [`nmp_app_chirp_seed_default_relays`] — production path: add the Chirp
//!   reference relay set.
//! * [`nmp_app_chirp_seed_relays_from_json`] — test-override path
//!   (`NMP_TEST_RELAYS`): parse a `[["url","role"],…]` JSON array and add each
//!   entry.
//!
//! Both are D6 fire-and-forget: a null app, malformed JSON, or an empty array
//! degrades to a `false` return rather than raising across the FFI. The Swift
//! shell reads the `NMP_TEST_RELAYS` env var (env plumbing stays in Swift) and
//! calls the JSON path; on a `false` return it falls back to the default path.

use std::ffi::{c_char, CStr};

use nmp_ffi::{nmp_app_add_relay, NmpApp};

/// Seed the Chirp reference relay set onto `app`.
///
/// Returns `true` when at least one relay was handed to the kernel, `false`
/// when `app` is null (D6). The canonical set comes from
/// `nmp_chirp_config::chirp_default_relay_bootstrap` — the same source the
/// Android shell, iOS shell, TUI, desktop shell, and generated web config use.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_seed_default_relays(app: *mut NmpApp) -> bool {
    if app.is_null() {
        return false;
    }
    let mut seeded = false;
    for entry in nmp_chirp_config::chirp_default_relay_bootstrap() {
        let (Ok(url), Ok(role)) = (
            std::ffi::CString::new(entry.url),
            std::ffi::CString::new(entry.role),
        ) else {
            continue;
        };
        // `nmp_app_add_relay` is the framework seam; it dedups against any
        // session-restored relay rows, so re-seeding an existing install is a
        // no-op on the kernel side.
        nmp_app_add_relay(app, url.as_ptr(), role.as_ptr());
        seeded = true;
    }
    seeded
}

/// Seed relays from a `[["url","role"],…]` JSON array (the `NMP_TEST_RELAYS`
/// override shape, identical to Android).
///
/// Returns `true` when the JSON was well-formed and at least one entry was
/// seeded, `false` when `app`/`json` is null, the JSON is malformed, or the
/// array is empty — the Swift caller must fall back to
/// [`nmp_app_chirp_seed_default_relays`] on `false`.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_seed_relays_from_json(
    app: *mut NmpApp,
    json: *const c_char,
) -> bool {
    if json.is_null() {
        return false;
    }
    // SAFETY: caller guarantees `json` (non-null, checked above) is a valid
    // nul-terminated C string for the duration of this call.
    let Ok(json) = (unsafe { CStr::from_ptr(json) }).to_str() else {
        return false;
    };
    seed_relays_from_json_str(app, json)
}

/// Parse-and-seed core, split out so unit tests can drive it with a `&str`
/// without manufacturing a C string. Returns the same `true`/`false` contract
/// as the C-ABI wrapper: `false` on a null app, malformed JSON, or an empty
/// array.
fn seed_relays_from_json_str(app: *mut NmpApp, json: &str) -> bool {
    if app.is_null() {
        return false;
    }
    let Ok(parsed) = serde_json::from_str::<Vec<[String; 2]>>(json) else {
        return false;
    };
    if parsed.is_empty() {
        return false;
    }
    let mut seeded = false;
    for entry in &parsed {
        let (Ok(url), Ok(role)) = (
            std::ffi::CString::new(entry[0].as_str()),
            std::ffi::CString::new(entry[1].as_str()),
        ) else {
            continue;
        };
        nmp_app_add_relay(app, url.as_ptr(), role.as_ptr());
        seeded = true;
    }
    seeded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    fn null_app() -> *mut NmpApp {
        ptr::null_mut()
    }

    #[test]
    fn default_seed_on_null_app_returns_false() {
        assert!(!nmp_app_chirp_seed_default_relays(null_app()));
    }

    #[test]
    fn json_seed_on_null_app_returns_false() {
        // Even valid JSON must short-circuit to false when the app is null.
        assert!(!seed_relays_from_json_str(
            null_app(),
            r#"[["wss://x","both"]]"#
        ));
    }

    #[test]
    fn json_seed_empty_array_returns_false() {
        // Empty array → caller must fall back to the defaults.
        assert!(!seed_relays_from_json_str(null_app(), "[]"));
    }

    #[test]
    fn json_seed_malformed_returns_false() {
        assert!(!seed_relays_from_json_str(null_app(), "not json"));
        assert!(!seed_relays_from_json_str(null_app(), "{}"));
        assert!(!seed_relays_from_json_str(null_app(), "[[]]")); // wrong inner shape
    }

    #[test]
    fn default_bootstrap_is_non_empty_and_well_formed() {
        // Guards the single-source-of-truth contract: the canonical set the
        // FFI seeds must be non-empty with non-empty url/role pairs.
        let bootstrap = nmp_chirp_config::chirp_default_relay_bootstrap();
        assert!(!bootstrap.is_empty(), "Chirp must ship ≥1 default relay");
        for entry in bootstrap {
            assert!(!entry.url.is_empty(), "relay URL must not be empty");
            assert!(!entry.role.is_empty(), "relay role must not be empty");
        }
    }
}
