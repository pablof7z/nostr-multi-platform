//! Regression guard for #1218 — Android x86_64 marmot APK cross-compile.
//!
//! `libsqlite3-sys/bundled-sqlcipher-vendored-openssl` vendors OpenSSL via
//! `openssl-sys/vendored` -> `openssl-src`. OpenSSL >= 3.5.0 ships the
//! post-quantum providers (`providers/ml_kem`, `ml_dsa`, `slh_dsa`), and the
//! `ml_kem` provider source fails to cross-compile for `x86_64-linux-android`
//! under NDK 26 / clang-17 (build exits 2 in the `providers/ml_kem` phase).
//! SQLCipher only ever uses AES + SHA, never the PQ providers.
//!
//! The fix pins `openssl-src` to the last pre-3.5 line (OpenSSL 3.4.1, which has
//! no PQ provider units) so the vendored build cross-compiles cleanly for ALL
//! shipped ABIs — arm64-v8a AND x86_64. These tests are hermetic string/range
//! checks over the manifest + lockfile (no NDK, no emulator), matching the
//! `bridge_parity.rs` posture.

const CARGO_TOML: &str = include_str!("../Cargo.toml");
const CARGO_LOCK: &str = include_str!("../Cargo.lock");

/// The manifest must pin `openssl-src` to an exact pre-3.5 line. A bare floor
/// (or no pin) lets the resolver pick `300.6.x+3.6.3`, which reintroduces the
/// ml_kem provider and re-breaks the x86_64 NDK build.
#[test]
fn manifest_pins_openssl_src_to_pre_pq_line() {
    assert!(
        CARGO_TOML.contains("openssl-src = { version = \"=300.4.2\""),
        "apps/chirp/crates/nmp-chirp-android-ffi/Cargo.toml must pin openssl-src to =300.4.2 \
         (OpenSSL 3.4.1, the last pre-ml_kem line that cross-compiles for \
         x86_64-linux-android under NDK 26) — see #1218",
    );

    // The pin must be gated behind `marmot` so non-Marmot builds (which never
    // pull libsqlite3-sys/openssl-sys) are unaffected.
    assert!(
        CARGO_TOML.contains("\"dep:openssl-src\""),
        "the openssl-src pin must be wired into the `marmot` feature via \
         dep:openssl-src so it only constrains the Marmot/vendored-OpenSSL path",
    );
}

/// The locked `openssl-src` must resolve to a `300.<=4>.x+3.<=4>.x` (pre-3.5)
/// version. Any `+3.5.` or higher carries the PQ providers and is rejected.
#[test]
fn lockfile_resolves_pre_3_5_openssl() {
    let block = CARGO_LOCK
        .split("[[package]]")
        .find(|p| p.contains("name = \"openssl-src\""))
        .expect("Cargo.lock must contain an openssl-src package entry (marmot graph)");

    let version_line = block
        .lines()
        .find(|l| l.trim_start().starts_with("version = "))
        .expect("openssl-src entry must declare a version");

    // Parse the bundled OpenSSL version from the `+x.y.z` build metadata, which
    // is the actual thing that gained the PQ providers in 3.5.0.
    let openssl_ver = version_line
        .split('+')
        .nth(1)
        .and_then(|s| s.trim_end_matches(['"', ' ']).rsplit('"').next())
        .map(|s| s.trim_matches('"').to_string())
        .unwrap_or_default();

    let mut parts = openssl_ver.split('.');
    let major: u32 = parts.next().unwrap_or("").parse().unwrap_or(0);
    let minor: u32 = parts.next().unwrap_or("").parse().unwrap_or(u32::MAX);

    assert!(
        major == 3 && minor <= 4,
        "openssl-src must vendor a pre-3.5 OpenSSL (no ml_kem/ml_dsa/slh_dsa \
         providers) so x86_64-linux-android cross-compiles under NDK 26; \
         resolved bundled OpenSSL = {openssl_ver:?} (from {version_line:?}) — see #1218",
    );
}
