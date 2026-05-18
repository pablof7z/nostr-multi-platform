//! `expand $var` — print the current expansion of a variable.

use std::collections::BTreeSet;

use crate::ast::VarName;
use crate::error::Result;
use crate::plan::expand_var_to_strings;
use crate::session::Session;

pub fn run(session: &Session, var: VarName) -> Result<()> {
    let empty: BTreeSet<String> = BTreeSet::new();
    let values = expand_var_to_strings(session, &var.0, &empty)?;
    println!("  ${} ({} values):", var.0, values.len());
    let preview = values.iter().take(20);
    for v in preview {
        println!("    {v}");
    }
    if values.len() > 20 {
        println!("    … ({} more)", values.len() - 20);
    }
    Ok(())
}
