//! `refresh [follows|mailboxes|all]` — invalidate caches. Does not
//! re-fetch eagerly; the next `req` will pick up the work.

use crate::ast::RefreshScope;
use crate::error::Result;
use crate::session::Session;

pub fn run(session: &mut Session, scope: RefreshScope) -> Result<()> {
    match scope {
        RefreshScope::Follows => {
            // Variable-expansion state only — independent of the lifecycle.
            session.follows_cache = None;
            println!("  cleared: follows_cache");
        }
        RefreshScope::Mailboxes => {
            // Re-arm implicit discovery: clear the lifecycle's probed set
            // AND drop the mailbox cache so the next `req` re-probes every
            // still-unknown author.
            let probed = session.lifecycle.probed_mailboxes().len();
            session.lifecycle.clear_probed_mailboxes();
            session.reset_lifecycle_cache_only();
            println!("  cleared: mailbox_cache + probed set ({probed} probed authors re-armed)");
        }
        RefreshScope::All => {
            let probed = session.lifecycle.probed_mailboxes().len();
            session.follows_cache = None;
            session.lifecycle.clear_probed_mailboxes();
            session.reset_lifecycle_cache_only();
            println!(
                "  cleared: follows_cache + mailbox_cache + probed set ({probed} probed re-armed)"
            );
        }
    }
    Ok(())
}
