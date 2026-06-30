use crate::support::is_comment;

const REJECTED_RELATION_TOKENS: &[&str] = &[
    "EventRelationSummary",
    "TargetInteractionCounts",
    "NoteRelationCounts",
    "TargetRelationCounts",
    "DefaultNoteRelationClassifier",
    "NoteRelationClassifier",
    "NoteRelationIndex",
    "RelationCountInterest",
    "RelationCount",
    "RelationKind",
    "visible_note_relations",
    "default_note_relation_classifier",
    "open_event_relations",
    "relation_counts",
    "interaction_counts",
];

const STORAGE_AGGREGATION_TOKENS: &[&str] = &["CounterKind"];
pub(crate) const ENGAGEMENT_NOUNS: &[&str] =
    &["replies", "reactions", "reposts", "zaps", "comments"];

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

pub(crate) fn contains_token(line: &str, token: &str) -> bool {
    let mut idx = 0;
    while let Some(pos) = line[idx..].find(token) {
        let start = idx + pos;
        let end = start + token.len();
        let before_ok = line[..start]
            .chars()
            .last()
            .is_none_or(|c| !is_ident_char(c));
        let after_ok = line[end..].chars().next().is_none_or(|c| !is_ident_char(c));
        if before_ok && after_ok {
            return true;
        }
        idx = end;
    }
    false
}

pub(crate) fn rejected_relation_token(trimmed: &str) -> Option<&'static str> {
    if is_comment(trimmed) {
        return None;
    }
    REJECTED_RELATION_TOKENS
        .iter()
        .copied()
        .find(|token| contains_token(trimmed, token))
}

pub(crate) fn storage_aggregation_token(trimmed: &str) -> Option<&'static str> {
    if is_comment(trimmed) {
        return None;
    }
    STORAGE_AGGREGATION_TOKENS
        .iter()
        .copied()
        .find(|token| contains_token(trimmed, token))
}

fn match_arm_for_kind(trimmed: &str, kind: &str) -> bool {
    let mut idx = 0;
    while let Some(pos) = trimmed[idx..].find(kind) {
        let start = idx + pos;
        let end = start + kind.len();
        let before_ok = trimmed[..start]
            .chars()
            .last()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
        let after = trimmed[end..].trim_start();
        let next_is_digit = trimmed[end..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit());
        let relation_rhs = after.contains("classify_reply")
            || after.contains("first_e_tag")
            || after.contains("CounterKind");
        if before_ok
            && !next_is_digit
            && relation_rhs
            && (after.starts_with("=>") || after.starts_with('|'))
        {
            return true;
        }
        idx = end;
    }
    false
}

pub(crate) fn storage_relation_kind_classifier(trimmed: &str) -> Option<&'static str> {
    if is_comment(trimmed) {
        return None;
    }
    if trimmed.contains("kind IN")
        && ["1", "6", "7", "9735"]
            .iter()
            .all(|kind| contains_token(trimmed, kind))
    {
        return Some("kind IN (1, 6, 7, 9735)");
    }
    [
        ("9735", "kind:9735 zap classifier"),
        ("7", "kind:7 reaction classifier"),
        ("6", "kind:6 repost classifier"),
        ("1", "kind:1 reply classifier"),
    ]
    .iter()
    .find_map(|(kind, label)| match_arm_for_kind(trimmed, kind).then_some(*label))
}

pub(crate) fn storage_nip10_marker_classifier(trimmed: &str) -> bool {
    if is_comment(trimmed) {
        return false;
    }
    (trimmed.contains("\"reply\"") || trimmed.contains("\"root\""))
        && (trimmed.contains("marker") || trimmed.contains("==") || trimmed.contains("!="))
}
