//! NIP-01 projection-type JSON-schema dumper.

#[cfg(feature = "codegen-schema")]
fn main() {
    println!("{}", nmp_nip01::codegen_schema::dump_pilot_schemas_json());
}

#[cfg(not(feature = "codegen-schema"))]
fn main() {
    eprintln!(
        "dump_nip01_projection_schemas requires the `codegen-schema` feature.\n\
         Re-run with: cargo run -p nmp-nip01 --features codegen-schema \
         --bin dump_nip01_projection_schemas"
    );
    std::process::exit(2);
}
