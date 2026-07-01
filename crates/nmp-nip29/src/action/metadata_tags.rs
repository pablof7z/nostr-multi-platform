//! Shared builder for the `["h", <local_id>]`-keyed tags carried on a
//! `kind:9002` (edit-metadata) event.
//!
//! NIP-29 subgroups (nips PR #2319) added a second author of kind:9002 — the
//! `SetParent` action — alongside the existing `CreateGroupAction`. To
//! avoid two hand-built 9002 tag paths (AGENTS.md "no fragmentation"), this
//! module is the single canonical constructor. `create.rs::metadata_plan`
//! builds a full metadata edit (name/about/picture/visibility/access +
//! optional parent); `set_parent.rs` builds a parent-only edit (all other
//! fields `None`, so the relay retains prior values per the 9002 spec).
//!
//! `None` for an optional field means "omit the tag" — the relay keeps the
//! prior value. An empty-string `name`/`about`/`picture` is treated as
//! `None` so callers can pass through trimmed user input without an extra
//! guard. `parent` follows the same rule: `None` (or empty) omits the tag,
//! which is how `SetParent` detaches a subgroup to root (the spec: "no
//! `parent` tag to detach").
//!
//! `children` is intentionally NOT built here. Reordering a parent's child
//! list is a parent-admin 9002 the relay maintains during adopt/detach; the
//! general child-list editor is a larger surface (see ADR-0060 / future
//! `edit_metadata`) and is out of scope for the subgroup increment.

use crate::action::{GroupAccess, GroupVisibility};

/// Construct the tag set for a `kind:9002` edit-metadata event for
/// `local_id`. Every optional field is omitted when `None`/empty so the
/// relay retains its prior value (NIP-29: absent tags keep prior values).
#[must_use]
pub fn metadata_edit_tags(
    local_id: &str,
    name: Option<&str>,
    about: Option<&str>,
    picture: Option<&str>,
    visibility: Option<GroupVisibility>,
    access: Option<GroupAccess>,
    parent: Option<&str>,
) -> Vec<Vec<String>> {
    let mut tags = vec![vec!["h".to_string(), local_id.to_string()]];
    if let Some(v) = name.map(str::trim).filter(|s| !s.is_empty()) {
        tags.push(vec!["name".to_string(), v.to_string()]);
    }
    if let Some(v) = about.map(str::trim).filter(|s| !s.is_empty()) {
        tags.push(vec!["about".to_string(), v.to_string()]);
    }
    if let Some(v) = picture.map(str::trim).filter(|s| !s.is_empty()) {
        tags.push(vec!["picture".to_string(), v.to_string()]);
    }
    if let Some(v) = visibility {
        tags.push(vec![visibility_tag_value(v).to_string()]);
    }
    if let Some(v) = access {
        tags.push(vec![access_tag_value(v).to_string()]);
    }
    if let Some(v) = parent.map(str::trim).filter(|s| !s.is_empty()) {
        tags.push(vec!["parent".to_string(), v.to_string()]);
    }
    tags
}

fn visibility_tag_value(v: GroupVisibility) -> &'static str {
    match v {
        GroupVisibility::Public => "public",
        GroupVisibility::Private => "private",
    }
}

fn access_tag_value(v: GroupAccess) -> &'static str {
    match v {
        GroupAccess::Open => "open",
        GroupAccess::Closed => "closed",
    }
}

/// Client-side guard for a `parent` value: the spec says relays MUST reject a
/// self-referential parent (a cycle of length one). Fail early at `start()`
/// rather than publishing a 9002 the relay will reject. Returns `Ok` for an
/// empty/`None` parent (root is always valid) and for a non-self parent.
pub fn validate_parent(parent: Option<&str>, own_local_id: &str) -> Result<(), String> {
    let Some(p) = parent.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    if p == own_local_id {
        return Err(format!(
            "parent must not equal the group's own local_id `{own_local_id}` (self-reference cycle)"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_metadata_edit_emits_all_tags_in_order() {
        let tags = metadata_edit_tags(
            "room",
            Some("Room"),
            Some("About"),
            Some("https://x/p.png"),
            Some(GroupVisibility::Private),
            Some(GroupAccess::Closed),
            Some("parent-room"),
        );
        assert_eq!(tags[0], vec!["h".to_string(), "room".to_string()]);
        assert_eq!(
            tags.iter()
                .filter(|t| t.first() == Some(&"name".to_string()))
                .count(),
            1
        );
        assert!(tags.iter().any(|t| t == &vec!["private".to_string()]));
        assert!(tags.iter().any(|t| t == &vec!["closed".to_string()]));
        assert!(tags
            .iter()
            .any(|t| t == &vec!["parent".to_string(), "parent-room".to_string()]));
    }

    #[test]
    fn parent_only_edit_omits_other_tags() {
        let tags = metadata_edit_tags("room", None, None, None, None, None, Some("parent"));
        // Only `h` + `parent`.
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0], vec!["h".to_string(), "room".to_string()]);
        assert_eq!(tags[1], vec!["parent".to_string(), "parent".to_string()]);
    }

    #[test]
    fn detach_parent_omits_the_tag() {
        // `SetParent` with `parent: None` → no `parent` tag (relay drops it
        // → root). Only the `h` tag is emitted.
        let tags = metadata_edit_tags("room", None, None, None, None, None, None);
        assert_eq!(tags, vec![vec!["h".to_string(), "room".to_string()]]);
    }

    #[test]
    fn empty_strings_collapse_to_omitted() {
        let tags = metadata_edit_tags("room", Some("   "), Some(""), None, None, None, Some(""));
        assert_eq!(tags, vec![vec!["h".to_string(), "room".to_string()]]);
    }

    #[test]
    fn validate_parent_rejects_self_reference() {
        assert!(validate_parent(Some("room"), "room").is_err());
    }

    #[test]
    fn validate_parent_accepts_non_self() {
        assert!(validate_parent(Some("other"), "room").is_ok());
    }

    #[test]
    fn validate_parent_accepts_none_and_empty() {
        assert!(validate_parent(None, "room").is_ok());
        assert!(validate_parent(Some(""), "room").is_ok());
        assert!(validate_parent(Some("   "), "room").is_ok());
    }
}
