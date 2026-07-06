//! Per-line rule dispatch for doctrine-lint.

use std::path::Path;

use crate::rules::{
    action_namespace, browser_runtime_boundary, d0, d10, d11, d13, d14, d15, d17, d19, d20, d21,
    d26, d27, d6, d7, d8, d9, deleted_defaults, feed_vocabulary, nip29_kind_blind, no_deprecated,
    no_raw_tap_reintroduction, product_raw_read, wasm_abi_only,
};
use crate::{allow, event_flow_gates, report, scope::is_doctrine_lint_source, walker::ScannedLine};

use super::{FileContext, FileState};

pub(super) fn scan_line(
    path: &Path,
    ctx: &FileContext,
    state: &mut FileState,
    sl: &ScannedLine<'_>,
    findings: &mut Vec<report::Finding>,
) {
    let in_d10_marked_fn = state.d10_tracker.in_marked_fn();
    state.d10_tracker.observe_line(sl.text);
    let in_uniffi_export_scope = state.d11_tracker.in_uniffi_export_scope();
    state.d11_tracker.observe_line(sl.text, false);
    if ctx.d12_in_scope {
        state.d12_line_is_comment.push(sl.is_comment);
    }
    state.d14_tracker.observe_line(sl.text, sl.is_comment);

    if !ctx.workspace_d8 && !ctx.d0_exempt {
        for hit in d0::check(sl.text, sl.is_comment) {
            emit_unless_allowed(path, sl, d0::ID, hit, allow::line_allows, findings);
        }
    }

    let d6_hits = d6::check(&mut state.d6_state, sl.text, sl.is_comment, sl.in_test_cfg);
    if !ctx.workspace_d8 && !ctx.d6_test_file && ctx.d6_in_scope {
        for hit in d6_hits {
            emit_unless_allowed(path, sl, d6::ID, hit, allow::line_allows, findings);
        }
    }

    if !ctx.workspace_d8 && ctx.d7_in_scope {
        for hit in d7::check(sl.text, sl.is_comment) {
            emit_unless_allowed(path, sl, d7::ID, hit, allow::line_allows, findings);
        }
    }

    if !ctx.workspace_d8 && ctx.d9_in_scope && !ctx.d6_test_file {
        for hit in d9::check(sl.text, sl.is_comment, sl.in_test_cfg) {
            emit_unless_allowed(
                path,
                sl,
                d9::ID,
                hit,
                allow::line_allows_with_reason,
                findings,
            );
        }
    }

    if !ctx.workspace_d8 && ctx.action_namespace_in_scope && !ctx.d6_test_file && !sl.in_test_cfg {
        for hit in action_namespace::check(sl.text, sl.is_comment) {
            emit_unless_allowed(
                path,
                sl,
                action_namespace::ID,
                hit,
                allow::line_allows_with_reason,
                findings,
            );
        }
    }

    if !ctx.workspace_d8
        && ctx.nip29_kind_blind_in_scope
        && !ctx.d6_test_file
        && !sl.in_test_cfg
        && !is_doctrine_lint_source(path)
    {
        for hit in nip29_kind_blind::check(sl.text, sl.is_comment) {
            emit_unless_allowed(
                path,
                sl,
                nip29_kind_blind::ID,
                hit,
                allow::line_allows_with_reason,
                findings,
            );
        }
    }

    if !ctx.workspace_d8 && ctx.d10_in_scope {
        for hit in d10::check(sl.text, sl.is_comment, in_d10_marked_fn) {
            if !d10::line_allows_d10(sl.text) {
                emit(path, sl, d10::ID, hit.0, hit.1, hit.2, findings);
            }
        }
    }

    if !ctx.workspace_d8 && !is_doctrine_lint_source(path) {
        for hit in d11::check(sl.text, sl.is_comment, in_uniffi_export_scope) {
            emit_unless_allowed(path, sl, d11::ID, hit, allow::line_allows, findings);
        }
    }

    if ctx.d13_part_a_in_scope {
        for hit in d13::check_part_a(sl.text, sl.is_comment, sl.in_test_cfg) {
            emit_unless_allowed(path, sl, d13::ID, hit, allow::line_allows, findings);
        }
    }
    if ctx.d13_part_b_in_scope {
        for hit in d13::check_part_b(sl.text, sl.is_comment) {
            emit_unless_allowed(path, sl, d13::ID, hit, allow::line_allows, findings);
        }
    }

    if !ctx.workspace_d8 && ctx.d14_in_scope && !ctx.d6_test_file && !sl.in_test_cfg {
        for hit in d14::check(sl.text, sl.is_comment, state.d14_tracker.current_struct()) {
            emit_unless_allowed(path, sl, d14::ID, hit, allow::line_allows, findings);
        }
    }

    if !ctx.workspace_d8 && ctx.d15_in_scope {
        for hit in d15::check(&mut state.d15_state, path, sl.text, sl.is_comment) {
            emit_unless_allowed(path, sl, d15::ID, hit, allow::line_allows, findings);
        }
    } else if ctx.d15_in_scope {
        let _ = d15::check(&mut state.d15_state, path, sl.text, sl.is_comment);
    }

    if !ctx.workspace_d8 && ctx.d17_in_scope && !ctx.d6_test_file && !sl.in_test_cfg {
        for hit in d17::check(sl.text, sl.is_comment) {
            emit_unless_allowed(path, sl, d17::ID, hit, allow::line_allows, findings);
        }
    }

    if !ctx.workspace_d8 && ctx.d19_in_scope && !ctx.d6_test_file {
        for hit in d19::check(sl.text, sl.is_comment, sl.in_test_cfg) {
            emit_unless_allowed(path, sl, d19::ID, hit, allow::line_allows, findings);
        }
    }

    if !ctx.workspace_d8 && ctx.d20_in_scope && !ctx.d6_test_file {
        for hit in d20::check(sl.text, sl.is_comment, sl.in_test_cfg) {
            emit_unless_allowed(path, sl, d20::ID, hit, allow::line_allows, findings);
        }
    }

    if !ctx.workspace_d8 && ctx.d21_in_scope && !ctx.d6_test_file {
        for hit in d21::check(sl.text, sl.is_comment, sl.in_test_cfg) {
            if !d21::line_allows_d21(sl.text) {
                emit(path, sl, d21::ID, hit.0, hit.1, hit.2, findings);
            }
        }
    }

    if !ctx.workspace_d8 && (ctx.d26_app_host_scope || ctx.d26_alk_scope) && !ctx.d6_test_file {
        for hit in d26::check(
            sl.text,
            ctx.d26_app_host_scope,
            ctx.d26_alk_scope,
            sl.is_comment,
            sl.in_test_cfg,
        ) {
            emit_unless_allowed(
                path,
                sl,
                d26::ID,
                hit,
                allow::line_allows_with_reason,
                findings,
            );
        }
    }

    if !ctx.workspace_d8 && ctx.d27_in_scope && !ctx.d6_test_file && !is_doctrine_lint_source(path)
    {
        let allowed = allow::line_allows(sl.text, d27::ID);
        for (col, message, suggested) in
            d27::findings_for_line(sl.text, sl.is_comment, sl.in_test_cfg, allowed)
        {
            emit(path, sl, d27::ID, col, message, suggested, findings);
        }
    }

    event_flow_gates::scan_line(
        &ctx.ef_scope,
        &mut state.ef_state,
        path,
        sl,
        ctx.workspace_d8,
        ctx.d6_test_file,
        findings,
    );

    if !ctx.workspace_d8
        && ctx.no_raw_tap_in_scope
        && !ctx.d6_test_file
        && !sl.in_test_cfg
        && !is_doctrine_lint_source(path)
    {
        for hit in no_raw_tap_reintroduction::check(
            sl.text,
            sl.is_comment,
            sl.in_test_cfg,
            no_raw_tap_reintroduction::in_sink_module(path),
        ) {
            emit_unless_allowed(
                path,
                sl,
                no_raw_tap_reintroduction::ID,
                hit,
                allow::line_allows_with_reason,
                findings,
            );
        }
    }

    if !ctx.workspace_d8
        && ctx.product_raw_read_in_scope
        && !ctx.d6_test_file
        && !sl.in_test_cfg
        && !is_doctrine_lint_source(path)
    {
        for hit in product_raw_read::check(sl.text, sl.is_comment, sl.in_test_cfg) {
            emit_unless_allowed(
                path,
                sl,
                product_raw_read::ID,
                hit,
                allow::line_allows_with_reason,
                findings,
            );
        }
    }

    if !ctx.workspace_d8
        && ctx.deleted_defaults_in_scope
        && !ctx.d6_test_file
        && !sl.in_test_cfg
        && !is_doctrine_lint_source(path)
    {
        for (col, message, suggested) in
            deleted_defaults::check(sl.text, sl.is_comment, sl.in_test_cfg)
        {
            emit(
                path,
                sl,
                deleted_defaults::ID,
                col,
                message,
                suggested,
                findings,
            );
        }
    }

    if !ctx.workspace_d8
        && ctx.feed_vocabulary_in_scope
        && !ctx.d6_test_file
        && !sl.in_test_cfg
        && !is_doctrine_lint_source(path)
    {
        for hit in feed_vocabulary::check(sl.text, sl.is_comment, sl.in_test_cfg) {
            emit_unless_allowed(
                path,
                sl,
                feed_vocabulary::ID,
                hit,
                allow::line_allows_with_reason,
                findings,
            );
        }
    }

    if !ctx.workspace_d8 && ctx.no_deprecated_in_scope && !is_doctrine_lint_source(path) {
        for (col, message, suggested) in no_deprecated::check(sl.text, sl.is_comment) {
            emit(
                path,
                sl,
                no_deprecated::ID,
                col,
                message,
                suggested,
                findings,
            );
        }
    }

    if !ctx.d8_test_file {
        for hit in d8::check_no_polling(sl.text, sl.is_comment, sl.in_test_cfg) {
            emit_unless_allowed(path, sl, d8::ID, hit, allow::line_allows, findings);
        }
    }

    if !ctx.workspace_d8 && ctx.wasm_abi_only_in_scope && !sl.in_test_cfg {
        for hit in wasm_abi_only::check(sl.text, sl.is_comment) {
            emit_unless_allowed(
                path,
                sl,
                wasm_abi_only::ID,
                hit,
                allow::line_allows,
                findings,
            );
        }
    }

    if !ctx.workspace_d8 && ctx.browser_runtime_boundary_in_scope && !sl.in_test_cfg {
        for hit in browser_runtime_boundary::check(sl.text, sl.is_comment) {
            emit_unless_allowed(
                path,
                sl,
                browser_runtime_boundary::ID,
                hit,
                allow::line_allows,
                findings,
            );
        }
    }
}

fn emit_unless_allowed(
    path: &Path,
    sl: &ScannedLine<'_>,
    rule: &'static str,
    hit: (usize, String, String),
    allows: fn(&str, &str) -> bool,
    findings: &mut Vec<report::Finding>,
) {
    if allows(sl.text, rule) {
        return;
    }
    emit(path, sl, rule, hit.0, hit.1, hit.2, findings);
}

fn emit(
    path: &Path,
    sl: &ScannedLine<'_>,
    rule: &'static str,
    col: usize,
    message: String,
    suggested: String,
    findings: &mut Vec<report::Finding>,
) {
    findings.push(report::Finding {
        rule,
        path: path.to_path_buf(),
        line: sl.line_no,
        col,
        message,
        suggested,
    });
}
