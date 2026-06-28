// D19 positive fixture — display formatting in a kernel projection builder.
// These lines must each fire a D19 finding.

fn build_profile_card(pubkey: &str) -> ProfileCard {
    let npub = crate::display::to_npub(pubkey); // D19: banned in projection file
    let ts = format_timestamp(row.created_at);  // D19: banned in projection file
    ProfileCard {
        pubkey: pubkey.to_string(),
        npub,
        created_at_display: ts,
    }
}

fn publish_error(kernel: &mut Kernel) {
    kernel.set_last_error_toast(Some("publish failed".to_string()));
}
