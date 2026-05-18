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
