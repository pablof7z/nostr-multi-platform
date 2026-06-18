//! Known-signer-catalog JSON dumper (#1493 P9).
//!
//! Thin shim over [`nmp_core::signer_catalog::dump_signer_catalog_json`]. The
//! catalog is the single Rust-owned source of truth for the known external
//! Nostr signer apps; `nmp-codegen` consumes this JSON to render the native
//! Kotlin/Swift/TS detection lists and the `AndroidManifest <queries>` / iOS
//! `LSApplicationQueriesSchemes` scheme declarations, so they can never drift
//! from Rust.
//!
//! ## Invocation
//!
//! ```sh
//! cargo run -p nmp-core --bin dump_signer_catalog > signer_catalog.json
//! ```
//!
//! Unlike `dump_projection_schemas`, this binary needs no `codegen-schema`
//! feature: the catalog is plain `serde::Serialize` data with no `schemars`
//! dependency.

fn main() {
    println!("{}", nmp_core::signer_catalog::dump_signer_catalog_json());
}
