//! Finding type + clippy-style report formatter.
//!
//! Each finding renders as one line:
//!
//! ```text
//! <path>:<line>:<col>: error[Dn]: <message>
//!     suggested: <fix>
//! ```
//!
//! The `error[Dn]:` shape is exactly clippy-parseable so CI annotators
//! ("review code in this PR") attach the lint as inline comments.

use std::path::PathBuf;

/// One lint finding emitted by a rule.
pub struct Finding {
    pub rule: &'static str, // e.g. "D0", "D6", "D7", "D8"
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub message: String,
    /// Suggested remediation. Printed as a `suggested:` indented line under
    /// the primary error.
    pub suggested: String,
}

impl Finding {
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "{}:{}:{}: error[{}]: {}",
            self.path.display(),
            self.line,
            self.col,
            self.rule,
            self.message
        ));
        if !self.suggested.is_empty() {
            s.push('\n');
            s.push_str(&format!("    suggested: {}", self.suggested));
        }
        s
    }
}

use std::process::ExitCode;

/// Sort findings, print them, and return the appropriate exit code.
///
/// Called from `main` after all roots have been scanned.
pub fn finish(
    root_count: usize,
    rule_label: &str,
    allow_findings: bool,
    mut all_findings: Vec<Finding>,
) -> ExitCode {
    // Stable order: by file, then by line, then by column.
    all_findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.col.cmp(&b.col))
    });

    for f in &all_findings {
        println!("{}", f.render());
    }

    if all_findings.is_empty() {
        eprintln!(
            "doctrine-lint: 0 findings across {} root(s) ({} clean).",
            root_count, rule_label
        );
        ExitCode::from(0)
    } else if allow_findings {
        eprintln!(
            "doctrine-lint: {} finding(s) (passing because --allow-findings).",
            all_findings.len()
        );
        ExitCode::from(0)
    } else {
        eprintln!("doctrine-lint: {} finding(s).", all_findings.len());
        ExitCode::from(1)
    }
}
