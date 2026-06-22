// `RemoteSignerHandle` was re-exported publicly here as a staged migration aid
// (issue #1720). Migration complete (issue #1772): all external importers now use
// `nmp_signer_iface` directly. The pub-external re-export is deleted; pub(crate)
// alias remains so nmp-core's own internals can keep using
// `crate::remote_signer::RemoteSignerHandle` without churn.
pub(crate) use nmp_signer_iface::RemoteSignerHandle;
