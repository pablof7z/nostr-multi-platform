//! `refresh [follows|mailboxes|all]` — invalidate caches. Does not
//! re-fetch eagerly; the next `req` will pick up the work.

use crate::ast::RefreshScope;
use crate::error::Result;
use crate::session::Session;

pub fn run(session: &mut Session, scope: RefreshScope) -> Result<()> {
    match scope {
        RefreshScope::Follows => {
            session.follows_cache = None;
            println!("  cleared: follows_cache");
        }
        RefreshScope::Mailboxes => {
            let n = session.mailbox_cache.len();
            session.mailbox_cache.clear();
            println!("  cleared: mailbox_cache ({n} entries)");
        }
        RefreshScope::All => {
            let n = session.mailbox_cache.len();
            session.follows_cache = None;
            session.mailbox_cache.clear();
            println!("  cleared: follows_cache + mailbox_cache ({n} entries)");
        }
    }
    Ok(())
}
