//! Per-file scan driver for doctrine-lint.
//!
//! `main.rs` owns CLI parsing and report finalization. This module owns the
//! Rust-file scan lifecycle: resolve file-level rule scopes, keep stateful
//! trackers aligned with the walker, and run the per-file D12 check after the
//! line pass.

mod line;

use std::path::Path;

use crate::cli::Config;
use crate::event_flow_gates;
use crate::report;
use crate::rules::{
    d0, d10, d11, d12, d13, d14, d15, d6, d7, d8, nip29_kind_blind, no_raw_tap_reintroduction,
    product_raw_read,
};
use crate::scope::{
    action_namespace_file_in_scope, d10_file_in_scope, d12_file_in_scope, d13_file_extra_in_scope,
    d14_file_in_scope, d15_file_in_scope, d16_file_in_scope, d17_file_in_scope, d19_file_in_scope,
    d20_file_in_scope, d21_file_in_scope, d26_active_local_keys_in_scope, d26_app_host_in_scope,
    d27_file_in_scope, d9_file_in_scope, is_doctrine_lint_source, is_nmp_testing_harness_bin,
};
use crate::{allow, walker};

pub(crate) fn scan_one_file(
    path: &Path,
    cfg: &Config,
    findings: &mut Vec<report::Finding>,
) -> std::io::Result<()> {
    let ctx = FileContext::resolve(path, cfg);
    let mut state = FileState::default();

    walker::scan_file(path, |sl| {
        line::scan_line(path, &ctx, &mut state, sl, findings);
    })?;

    if !ctx.workspace_d8 && ctx.d12_in_scope {
        let body = std::fs::read_to_string(path)?;
        for hit in d12::scan_file(&body, &state.d12_line_is_comment) {
            let marker_line = body.lines().nth(hit.line.saturating_sub(1)).unwrap_or("");
            if allow::line_allows(marker_line, d12::ID) {
                continue;
            }
            findings.push(report::Finding {
                rule: d12::ID,
                path: path.to_path_buf(),
                line: hit.line,
                col: hit.col,
                message: hit.message,
                suggested: hit.suggested,
            });
        }
    }

    Ok(())
}

pub(super) struct FileContext {
    workspace_d8: bool,
    d0_exempt: bool,
    d6_test_file: bool,
    d8_test_file: bool,
    no_raw_tap_in_scope: bool,
    product_raw_read_in_scope: bool,
    action_namespace_in_scope: bool,
    nip29_kind_blind_in_scope: bool,
    d7_in_scope: bool,
    d8_in_scope: bool,
    d9_in_scope: bool,
    d10_in_scope: bool,
    d12_in_scope: bool,
    d13_part_a_in_scope: bool,
    d13_part_b_in_scope: bool,
    d14_in_scope: bool,
    d15_in_scope: bool,
    d16_in_scope: bool,
    d17_in_scope: bool,
    d19_in_scope: bool,
    d20_in_scope: bool,
    d21_in_scope: bool,
    d26_app_host_scope: bool,
    d26_alk_scope: bool,
    d27_in_scope: bool,
    ef_scope: event_flow_gates::FileScope,
}

impl FileContext {
    fn resolve(path: &Path, cfg: &Config) -> Self {
        let d6_test_file = d6::file_is_test_only(path);
        let d13_part_a_in_scope = {
            let default = d13::file_in_part_a_default(path);
            let extra = d13_file_extra_in_scope(path, &cfg.d13_extra_scopes);
            let marker = !is_doctrine_lint_source(path)
                && std::fs::read_to_string(path)
                    .map(|s| s.contains(d13::PART_A_MARKER))
                    .unwrap_or(false);
            default || extra || marker
        };

        Self {
            workspace_d8: cfg.workspace_d8,
            d0_exempt: d0::file_is_exempt(path),
            d6_test_file,
            d8_test_file: d6_test_file && !(cfg.workspace_d8 && is_nmp_testing_harness_bin(path)),
            no_raw_tap_in_scope: no_raw_tap_reintroduction::file_in_scope(path),
            product_raw_read_in_scope: product_raw_read::file_in_scope(path),
            action_namespace_in_scope: action_namespace_file_in_scope(path),
            nip29_kind_blind_in_scope: nip29_kind_blind::file_in_scope(path),
            d7_in_scope: d7::file_in_scope(path),
            d8_in_scope: d8::file_in_scope(path, &cfg.d8_extra_scopes),
            d9_in_scope: d9_file_in_scope(path, &cfg.d9_extra_scopes),
            d10_in_scope: d10_file_in_scope(path, &cfg.d10_extra_scopes),
            d12_in_scope: d12_file_in_scope(path, &cfg.d12_extra_scopes),
            d13_part_a_in_scope,
            d13_part_b_in_scope: d13::file_in_part_b_scope(path),
            d14_in_scope: d14_file_in_scope(path, &cfg.d14_extra_scopes),
            d15_in_scope: d15_file_in_scope(path, &cfg.d15_extra_scopes),
            d16_in_scope: d16_file_in_scope(path, &cfg.d16_extra_scopes),
            d17_in_scope: d17_file_in_scope(path, &cfg.d17_extra_scopes),
            d19_in_scope: d19_file_in_scope(path, &cfg.d19_extra_scopes),
            d20_in_scope: d20_file_in_scope(path, &cfg.d20_extra_scopes),
            d21_in_scope: d21_file_in_scope(path, &cfg.d21_extra_scopes),
            d26_app_host_scope: d26_app_host_in_scope(path, &cfg.d26_extra_scopes),
            d26_alk_scope: d26_active_local_keys_in_scope(path, &cfg.d26_extra_scopes),
            d27_in_scope: d27_file_in_scope(path, &cfg.d27_extra_scopes),
            ef_scope: event_flow_gates::FileScope::resolve(
                path,
                &cfg.d23_extra_scopes,
                &cfg.d24_extra_scopes,
                &cfg.d25_extra_scopes,
            ),
        }
    }
}

#[derive(Default)]
pub(super) struct FileState {
    ef_state: event_flow_gates::ScanState,
    d6_state: d6::State,
    d8_tracker: d8::HotPathTracker,
    d10_tracker: d10::PrivatePublishTracker,
    d11_tracker: d11::FnTracker,
    d14_tracker: d14::StructTracker,
    d15_state: d15::State,
    d12_line_is_comment: Vec<bool>,
}
