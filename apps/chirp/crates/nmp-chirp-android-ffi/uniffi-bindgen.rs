// UniFFI bindgen entry-point (M14-0 / issue #2129).
//
// Pinned via `required-features = ["bindgen-cli"]` in Cargo.toml so this
// binary is only built when explicitly requested.  The uniffi version here
// is identical to the runtime dep in [dependencies]; the two must stay in
// sync (see Cargo.toml comment above the uniffi entry).
//
// Usage:
//   cargo run --bin uniffi-bindgen \
//     --manifest-path apps/chirp/crates/nmp-chirp-android-ffi/Cargo.toml \
//     --features bindgen-cli -- \
//     generate --library target/debug/libnmp_android_ffi.dylib \
//     --crate nmp_chirp_android_ffi --language kotlin \
//     --config uniffi.toml \
//     --out-dir apps/chirp/android/app/src/main/java --no-format
fn main() {
    uniffi::uniffi_bindgen_main()
}
