//! `expand $var` — print the current expansion of a variable.
//!
//! `expand` is a diagnostic tool: the user wants to *see* the set. For
//! `$follows` we therefore distinguish two cases:
//!
//! - **No seed set** → genuine error (`set-seed` first).
//! - **Seed set, follows not yet cached** → do the kind:3 fetch on demand
//!   and print the resolved set. (The old code errored "requires a seed…"
//!   here even though the seed WAS set — the real issue was just an
//!   unfetched kind:3. Fixed.)

use std::collections::BTreeSet;

use crate::ast::VarName;
use crate::error::Result;
use crate::plan::expand_var_to_strings;
use crate::session::Session;

pub fn run(session: &mut Session, var: VarName) -> Result<()> {
    // `$follows` on a set-but-uncached seed: trigger the fetch so the
    // diagnostic actually shows the set instead of erroring.
    if var.0 == "follows" && session.seed_hex.is_some() && session.follows_cache.is_none() {
        let r = crate::discovery::fetch_follows(session)?;
        if r.cached {
            println!("  $follows: cached ({} follows)", r.follows.len());
        } else {
            println!(
                "  $follows: fetched {} follows in {}ms",
                r.follows.len(),
                r.elapsed.as_millis()
            );
        }
    }

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
