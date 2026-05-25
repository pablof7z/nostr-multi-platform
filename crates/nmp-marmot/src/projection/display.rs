//! Free-form metadata fallbacks for the Marmot snapshot.
//!
//! These helpers cover empty-name fallbacks and pluralisation for
//! free-form group metadata (group `name`, invite chip). They are NOT
//! pubkey- or timestamp-formatters — aim.md §2 bans the latter from
//! projection paths, but free-form metadata fallbacks are legitimate
//! protocol-level decisions about how to surface a `name` field with no
//! kind-defined empty-string semantics.
//!
//! Pure functions over snapshot inputs — no state, no I/O. They live in
//! the projection module (not the substrate) because they only serve the
//! FFI-payload layer.

/// First 2 ASCII letters of `name`, uppercased; falls back to `"?"` on
/// empty input. Used for the group avatar tile.
#[must_use]
pub fn initials(name: &str) -> String {
    let mut chars = name.chars().filter(|c| !c.is_whitespace());
    let a = chars.next();
    let b = chars.next();
    match (a, b) {
        (Some(x), Some(y)) => format!("{}{}", x, y).to_uppercase(),
        (Some(x), None) => x.to_uppercase().to_string(),
        _ => "?".to_string(),
    }
}

/// `Some("1 invite")` / `Some("3 invites")` / `None`. Drives the
/// top-of-list invite chip.
#[must_use]
pub fn invites_chip_label(count: usize) -> Option<String> {
    match count {
        0 => None,
        1 => Some("1 invite".to_string()),
        n => Some(format!("{n} invites")),
    }
}

/// Empty-name fallback. Avoids `name.isEmpty ? "Untitled group" : name`
/// in the shell.
#[must_use]
pub fn group_display_name(name: &str) -> String {
    if name.is_empty() {
        "Untitled group".to_string()
    } else {
        name.to_string()
    }
}

/// Empty-name fallback for a welcome / invite row.
#[must_use]
pub fn welcome_display_name(name: &str) -> String {
    if name.is_empty() {
        "Group invite".to_string()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_basic() {
        assert_eq!(initials("Trusted Circle"), "TR");
        assert_eq!(initials("a"), "A");
        assert_eq!(initials(""), "?");
        assert_eq!(initials("   spaces"), "SP");
    }

    #[test]
    fn invites_chip_label_pluralises() {
        assert_eq!(invites_chip_label(0), None);
        assert_eq!(invites_chip_label(1), Some("1 invite".to_string()));
        assert_eq!(invites_chip_label(4), Some("4 invites".to_string()));
    }

    #[test]
    fn group_display_name_falls_back_on_empty() {
        assert_eq!(group_display_name(""), "Untitled group");
        assert_eq!(group_display_name("Friends"), "Friends");
    }

    #[test]
    fn welcome_display_name_falls_back_on_empty() {
        assert_eq!(welcome_display_name(""), "Group invite");
        assert_eq!(welcome_display_name("Crew"), "Crew");
    }
}
