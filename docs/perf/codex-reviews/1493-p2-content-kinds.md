# Codex review — fix/1493-p2-content-kinds (#1493)

Diff: name the bare kind literals in nmp-content's two per-kind decision
points (`sniff_mode_from_kind`, `resolve_embed_projection`).

## Verdict: No blocking issues.

- No behavioral change: match arms dispatch the same values to the same
  projections/modes.
- Constants match originals exactly: 30_023 (KIND_LONG_FORM_ARTICLE),
  30_024 (KIND_LONG_FORM_DRAFT), 30_818 (KIND_WIKI_ARTICLE),
  0 (KIND_PROFILE_METADATA), 1 (KIND_SHORT_NOTE), 9_802 (KIND_HIGHLIGHT).
- Match-arm ordering and `_` fallback preserved.
- `cargo check -p nmp-content` passes; no unused-import / dead-const issue.
