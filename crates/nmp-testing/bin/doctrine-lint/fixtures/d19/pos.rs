// D19 positive fixture — presentation formatting in a kernel projection
// builder. These lines must each fire a D19 finding. Canonical bech32 codec
// use (`to_npub`) is NOT part of this fixture — see neg.rs, where it is
// proven compliant (#3113, ADR-0077).

fn build_profile_card(pubkey: &str) -> ProfileCard {
    let npub_short = crate::display::short_npub(pubkey); // D19: banned (presentation truncation) in projection file
    // Catch-by-default (#3113, ADR-0077): a `crate::display::` helper that is
    // NOT in the codec allowlist — even one nobody has ever added to a banned
    // list — must fire. `avatar_shape` is a stand-in for "the next
    // presentation helper someone adds tomorrow".
    let shape = crate::display::avatar_shape(pubkey); // D19: banned by default (not a codec)
    let ts = format_timestamp(row.created_at);  // D19: banned in projection file
    ProfileCard {
        pubkey: pubkey.to_string(),
        npub_short,
        shape,
        created_at_display: ts,
    }
}

fn publish_error(kernel: &mut Kernel) {
    kernel.set_last_error_toast(Some("publish failed".to_string()));
}
