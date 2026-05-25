mod helpers;

use helpers::{lock_sha_for_path, nmp, sha256_hex_of, TempDir};
use std::fs;

#[test]
fn cross_platform_tui_content_view() {
    let tmp = TempDir::new("e2e-tui-content");

    let add = nmp(tmp.path(), &["add", "component", "tui/content-view"]);
    assert!(
        add.status.success(),
        "add tui/content-view failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    let nc = tmp.path().join("src/components/nostr_content");
    let files = [
        (
            nc.join("content_tree_wire.rs"),
            "src/components/nostr_content/content_tree_wire.rs",
        ),
        (
            nc.join("content_render_data.rs"),
            "src/components/nostr_content/content_render_data.rs",
        ),
        (
            nc.join("ratatui_text_wrap.rs"),
            "src/components/nostr_content/ratatui_text_wrap.rs",
        ),
        (
            nc.join("nostr_mention_chip.rs"),
            "src/components/nostr_content/nostr_mention_chip.rs",
        ),
        (
            nc.join("nostr_media_grid.rs"),
            "src/components/nostr_content/nostr_media_grid.rs",
        ),
        (
            nc.join("nostr_quote_card.rs"),
            "src/components/nostr_content/nostr_quote_card.rs",
        ),
        (
            nc.join("nostr_content_view.rs"),
            "src/components/nostr_content/nostr_content_view.rs",
        ),
    ];
    for (path, _) in &files {
        assert!(path.exists(), "expected TUI file: {}", path.display());
    }

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    for id in &[
        "tui/content-core",
        "tui/content-mention-chip",
        "tui/content-media-grid",
        "tui/content-quote-card",
        "tui/content-view",
    ] {
        assert!(
            lock.contains(&format!("id = \"{id}\"")),
            "lock missing TUI component {id}: {lock}"
        );
    }

    for (path, target) in &files {
        let on_disk = fs::read_to_string(path).unwrap();
        let actual = lock_sha_for_path(&lock, target)
            .unwrap_or_else(|| panic!("lock missing sha for {target}: {lock}"));
        assert_eq!(
            actual,
            sha256_hex_of(&on_disk),
            "TUI lock sha mismatch for {target}"
        );
    }
}

#[test]
fn cross_platform_tui_user_card() {
    let tmp = TempDir::new("e2e-tui-user");

    let add = nmp(tmp.path(), &["add", "component", "tui/user-card"]);
    assert!(
        add.status.success(),
        "add tui/user-card failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    let nu = tmp.path().join("src/components/nostr_user");
    let files = [
        (
            nu.join("profile_wire.rs"),
            "src/components/nostr_user/profile_wire.rs",
        ),
        (
            nu.join("nostr_avatar.rs"),
            "src/components/nostr_user/nostr_avatar.rs",
        ),
        (
            nu.join("nostr_profile_name.rs"),
            "src/components/nostr_user/nostr_profile_name.rs",
        ),
        (
            nu.join("nostr_nip05_badge.rs"),
            "src/components/nostr_user/nostr_nip05_badge.rs",
        ),
        (
            nu.join("nostr_user_card.rs"),
            "src/components/nostr_user/nostr_user_card.rs",
        ),
    ];
    for (path, _) in &files {
        assert!(path.exists(), "expected TUI file: {}", path.display());
    }

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    for id in &[
        "tui/user-core",
        "tui/user-avatar",
        "tui/user-name",
        "tui/user-nip05",
        "tui/user-card",
    ] {
        assert!(
            lock.contains(&format!("id = \"{id}\"")),
            "lock missing TUI component {id}: {lock}"
        );
    }

    for (path, target) in &files {
        let on_disk = fs::read_to_string(path).unwrap();
        let actual = lock_sha_for_path(&lock, target)
            .unwrap_or_else(|| panic!("lock missing sha for {target}: {lock}"));
        assert_eq!(
            actual,
            sha256_hex_of(&on_disk),
            "TUI lock sha mismatch for {target}"
        );
    }
}
