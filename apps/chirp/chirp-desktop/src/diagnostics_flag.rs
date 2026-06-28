const DIAGNOSTICS_ENV: &str = "NMP_DESKTOP_DIAGNOSTICS";

pub(crate) fn enabled() -> bool {
    enabled_from(
        cfg!(debug_assertions),
        std::env::var_os(DIAGNOSTICS_ENV).is_some(),
    )
}

fn enabled_from(debug_build: bool, env_present: bool) -> bool {
    debug_build || env_present
}

#[cfg(test)]
mod tests {
    use super::enabled_from;

    #[test]
    fn diagnostics_are_available_in_debug_builds() {
        assert!(enabled_from(true, false));
    }

    #[test]
    fn diagnostics_are_available_when_flagged() {
        assert!(enabled_from(false, true));
    }

    #[test]
    fn diagnostics_are_hidden_in_unflagged_release_builds() {
        assert!(!enabled_from(false, false));
    }
}
