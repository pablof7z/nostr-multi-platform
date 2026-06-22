// `SignedEvent`, `UnsignedEvent`, and `SigningError` were re-exported here as a
// public staged migration aid (issue #1720). Migration complete (issue #1772): all
// external importers now use `nmp_signer_iface` directly. The pub-external re-export
// is deleted; pub(crate) aliases remain so nmp-core's own internals can keep using
// `crate::substrate::{SignedEvent, UnsignedEvent}` without churn.
// (`SigningError` is no longer needed internally either — removed.)
pub(crate) use nmp_signer_iface::{SignedEvent, UnsignedEvent};
