//! Gate checker per `docs/design/ffi-hardening/gates.md` G-S1..G-S5.
//!
//! A `Gate` is a named assertion: measured value op threshold.
//! Collect gates into a `Vec<Gate>` during a scenario run, then call
//! `Gate::all_pass` to determine whether the scenario passed the
//! numeric exit criteria.
//!
//! D6: any gate failure is surfaced here as structured data, not as an
//! opaque error string, so the caller can report, aggregate, and diff.

use serde::Serialize;

/// Operator for a gate comparison.
#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) enum GateOp {
    /// `measured <= threshold`
    #[serde(rename = "<=")]
    Lte,
    /// `measured >= threshold`
    #[serde(rename = ">=")]
    Gte,
    /// `measured == threshold`
    #[serde(rename = "==")]
    Eq,
}

/// One numeric assertion from the gate table.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Gate {
    pub(crate) name: String,
    pub(crate) op: GateOp,
    pub(crate) threshold: f64,
    pub(crate) measured: f64,
    pub(crate) passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) note: Option<String>,
}

impl Gate {
    pub(crate) fn lte(name: impl Into<String>, measured: f64, threshold: f64) -> Self {
        let passed = measured <= threshold;
        Gate {
            name: name.into(),
            op: GateOp::Lte,
            threshold,
            measured,
            passed,
            note: None,
        }
    }

    pub(crate) fn gte(name: impl Into<String>, measured: f64, threshold: f64) -> Self {
        let passed = measured >= threshold;
        Gate {
            name: name.into(),
            op: GateOp::Gte,
            threshold,
            measured,
            passed,
            note: None,
        }
    }

    pub(crate) fn eq(name: impl Into<String>, measured: f64, threshold: f64) -> Self {
        let passed = (measured - threshold).abs() < f64::EPSILON;
        Gate {
            name: name.into(),
            op: GateOp::Eq,
            threshold,
            measured,
            passed,
            note: None,
        }
    }

    pub(crate) fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    pub(crate) fn all_pass(gates: &[Gate]) -> bool {
        gates.iter().all(|g| g.passed)
    }
}

/// Markdown row for one gate. Used by `report::markdown_gates_table`.
impl Gate {
    pub(crate) fn markdown_row(&self) -> String {
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
