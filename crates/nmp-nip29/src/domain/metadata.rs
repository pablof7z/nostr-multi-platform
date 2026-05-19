//! Relay-signed metadata `DomainModule` impls (39000–39003).
//!
//! Per `moderation.md` §4: 39000 establishes the TOFU pin; 39001/39002/39003
//! are held in a quarantine buffer until a 39000 has landed for the same
//! `(host_relay_url, local_id)` to defeat the user-signed-spoofing vector.
//!
//! Per `moderation.md` §5: `GroupAdmins` (39001) and `GroupMembers` (39002)
//! are the **only** canonical membership records — user-signed 9000/9001
//! moderation actions never mutate them; they flip only on the relay's
//! republished 39001/39002 snapshot.

use nmp_core::substrate::{DomainIndex, DomainMigration, DomainModule};


macro_rules! noop_migrations {
    () => {
        fn migrations() -> Vec<DomainMigration> {
            Vec::new()
        }
        fn indexes() -> Vec<DomainIndex> {
            // Primary key (group composite-key prefix) is implicit; reverse
            // indexes for cross-protocol joins live at the kernel substrate
            // level per the composite-key reverse index (ADR-0001).
            Vec::new()
        }
    };
}

/// Kind 39000 — group identity / metadata snapshot.
pub struct GroupModule;
impl DomainModule for GroupModule {
    const NAMESPACE: &'static str = "nip29.group";
    const SCHEMA_VERSION: u32 = 1;
    noop_migrations!();}

/// Kind 39001 — admin set snapshot.
pub struct GroupAdminsModule;
impl DomainModule for GroupAdminsModule {
    const NAMESPACE: &'static str = "nip29.group_admins";
    const SCHEMA_VERSION: u32 = 1;
    noop_migrations!();}

/// Kind 39002 — member set snapshot.
pub struct GroupMembersModule;
impl DomainModule for GroupMembersModule {
    const NAMESPACE: &'static str = "nip29.group_members";
    const SCHEMA_VERSION: u32 = 1;
    noop_migrations!();}

/// Kind 39003 — relay-declared role catalog (optional; absence = empty).
pub struct GroupRolesModule;
impl DomainModule for GroupRolesModule {
    const NAMESPACE: &'static str = "nip29.group_roles";
    const SCHEMA_VERSION: u32 = 1;
    noop_migrations!();}
