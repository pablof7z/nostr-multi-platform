pub(crate) struct RuleBBaseline {
    pub(crate) path: &'static str,
    pub(crate) max_hits: usize,
    pub(crate) issue: &'static str,
    pub(crate) reason: &'static str,
}

/// Rule B has no grandfathered production debt. Do NOT add new entries; delete
/// rejected relation/count surfaces instead.
pub(crate) const RULE_B_BASELINE: &[RuleBBaseline] = &[];

pub(crate) fn baseline_for(path: &str) -> Option<&'static RuleBBaseline> {
    RULE_B_BASELINE.iter().find(|entry| entry.path == path)
}
