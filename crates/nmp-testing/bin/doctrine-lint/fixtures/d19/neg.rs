// D19 negative fixture — projection builder that correctly sends raw data.
// These lines must NOT fire a D19 finding.

fn build_profile_card(pubkey: &str, created_at: u64) -> ProfileCard {
    // Correct: raw hex pubkey — no bech32 encoding.
    // Correct: raw Unix seconds — no format_timestamp call.
    ProfileCard {
        pubkey: pubkey.to_string(),
        created_at,
    }
}

/// #3113 / ADR-0077 — `to_npub` is a canonical, lossless hex<->bech32 codec,
/// not display formatting. Calling it directly in projection-builder prod
/// code (outside `#[cfg(test)]`) must NOT fire D19.
fn build_npub_for_wire(pubkey: &str) -> String {
    crate::display::to_npub(pubkey)
}

fn publish_error(kernel: &mut Kernel) {
    kernel.set_last_error_token(&crate::ui_token::UiToken::error(
        crate::ui_token::codes::PUBLISH_SIGN_FAILED,
        "publish failed",
    ));
}

// Calling display helpers in the display module itself is fine — not in scope.
// (This file is placed under /fixtures/ so it is never matched by the scope
// check — just confirming the shape of compliant code.)
#[cfg(test)]
mod tests {
    fn test_display() {
        // Test code may use crate::display:: without triggering D19.
        let _ = crate::display::to_npub("aabbcc");
        let _ = format_timestamp(12345678);
    }
}
