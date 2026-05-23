//! V6 Stage 1 — projection-type JSON-schema dumper.
//!
//! Thin shim around `nmp_core::codegen_schema::dump_pilot_schemas_json`.
//! The interesting logic lives in `crates/nmp-core/src/codegen_schema.rs`
//! (which has crate-private access to the projection types) — this binary
//! just prints the resulting JSON to stdout.
//!
//! ## Invocation
//!
//! ```sh
//! cargo run -p nmp-core --features codegen-schema \
//!     --bin dump_projection_schemas > schemas.json
//! ```
//!
//! Without the `codegen-schema` feature the binary still compiles (so
//! `cargo check -p nmp-core` keeps scanning every target) but exits with a
//! clear error pointing at the missing feature, rather than failing to
//! parse the file at all.

#[cfg(feature = "codegen-schema")]
fn main() {
    println!("{}", nmp_core::codegen_schema::dump_pilot_schemas_json());
}

#[cfg(not(feature = "codegen-schema"))]
fn main() {
    eprintln!(
        "dump_projection_schemas requires the `codegen-schema` feature.\n\
         Re-run with: cargo run -p nmp-core --features codegen-schema \
         --bin dump_projection_schemas"
    );
    std::process::exit(2);
}
