//! Shared gate-assertion type for test and perf harnesses.
//!
//! Both `ffi-stress` and `firehose-bench` emit JSON reports whose gate entries
//! use this type, giving CI tooling a single parseable schema regardless of
//! which harness produced the output.
//!
//! A [`Gate`] is a named numeric assertion: `measured <op> threshold`. After a
//! scenario run the driver collects `Vec<Gate>`, calls [`Gate::all_pass`] for
//! the scenario-level PASS/FAIL verdict, and serialises the gates array into
//! the top-level report.

use serde::Serialize;

/// Version tag written into every harness report's top-level JSON.
/// Both `ffi-stress` and `firehose-bench` must set `schema_version` to this
/// constant so consumers can detect incompatible format changes.
pub const SCHEMA_VERSION: u32 = 1;

/// Comparison operator for a gate assertion.
#[derive(Clone, Copy, Debug, Serialize)]
pub enum GateOp {
    /// `measured <= threshold`
    #[serde(rename = "<=")]
    Lte,
    /// `measured >= threshold`
    #[serde(rename = ">=")]
    Gte,
    /// `|measured - threshold| < ε` (floating-point equality)
    #[serde(rename = "==")]
    Eq,
}

/// One numeric gate assertion from a harness scenario.
///
/// Serialize as a JSON object with `name`, `op`, `threshold`, `measured`,
/// `passed`, and an optional `note`. Both `ffi-stress` and `firehose-bench`
/// use this type verbatim so their `gates` arrays are schema-compatible.
#[derive(Clone, Debug, Serialize)]
pub struct Gate {
    pub name: String,
    pub op: GateOp,
    pub threshold: f64,
    pub measured: f64,
    pub passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Gate {
    /// PASS iff `measured <= threshold`.
    pub fn lte(name: impl Into<String>, measured: f64, threshold: f64) -> Self {
        Gate {
            name: name.into(),
            op: GateOp::Lte,
            threshold,
            measured,
            passed: measured <= threshold,
            note: None,
        }
    }

    /// PASS iff `measured >= threshold`.
    pub fn gte(name: impl Into<String>, measured: f64, threshold: f64) -> Self {
        Gate {
            name: name.into(),
            op: GateOp::Gte,
            threshold,
            measured,
            passed: measured >= threshold,
            note: None,
        }
    }

    /// PASS iff `|measured - threshold| < f64::EPSILON`.
    pub fn eq(name: impl Into<String>, measured: f64, threshold: f64) -> Self {
        Gate {
            name: name.into(),
            op: GateOp::Eq,
            threshold,
            measured,
            passed: (measured - threshold).abs() < f64::EPSILON,
            note: None,
        }
    }

    /// Attach a one-line human-readable note (e.g. spec citation, context).
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// `true` iff every gate in `gates` has `passed == true`.
    pub fn all_pass(gates: &[Gate]) -> bool {
        gates.iter().all(|g| g.passed)
    }

    /// One Markdown table row for use in report generators.
    pub fn markdown_row(&self) -> String {
        let result = if self.passed { "PASS" } else { "FAIL" };
        let op_str = match self.op {
            GateOp::Lte => "<=",
            GateOp::Gte => ">=",
            GateOp::Eq => "==",
        };
        let note = self
            .note
            .as_deref()
            .map(|n| format!(" ({n})"))
            .unwrap_or_default();
        format!(
            "| {} | {} {:.4} | {:.4} | {} |{}\n",
            self.name, op_str, self.threshold, self.measured, result, note
        )
    }
}
