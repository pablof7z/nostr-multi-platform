//! `set-budget [max_connections=N] [max_per_user=N] [wall=Ns]` — patch
//! the planner-selector + fanout-wall budgets.

use crate::ast::BudgetPatch;
use crate::error::Result;
use crate::session::Session;

pub fn run(session: &mut Session, patch: BudgetPatch) -> Result<()> {
    if let Some(n) = patch.max_connections {
        session.max_connections = n;
    }
    if let Some(n) = patch.max_per_user {
        session.max_per_user = n;
    }
    if let Some(d) = patch.wall {
        session.wall = d;
    }
    println!(
        "  budget: max_connections={} max_per_user={} wall={:?}",
        session.max_connections, session.max_per_user, session.wall
    );
    Ok(())
}
