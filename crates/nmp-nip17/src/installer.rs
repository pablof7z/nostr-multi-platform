use nmp_core::substrate::{
    ActionRegistrar, ConfiguredRelaysChangeRegistrar, DmInboxRelayRegistrar, HostCapabilities,
    IdentityChangeRegistrar, IngestParserRegistrar, PublishPolicyRegistrar,
    SnapshotProjectionRegistrar,
};

#[derive(Clone, Debug, Default)]
pub struct Config {}

#[derive(Clone, Debug, Default)]
pub struct Handles {}

pub fn register(
    app: &mut (impl ActionRegistrar
              + DmInboxRelayRegistrar
              + IngestParserRegistrar
              + HostCapabilities
              + IdentityChangeRegistrar
              + ConfiguredRelaysChangeRegistrar
              + PublishPolicyRegistrar
              + SnapshotProjectionRegistrar),
    _config: Config,
) -> Result<Handles, nmp_core::substrate::RegistrationError> {
    crate::declare_publish_policy(app).map_err(|_| nmp_core::substrate::RegistrationError {
        namespace: "publish_policy",
        prior_provider: "nmp-core::publish",
        new_provider: "nmp-nip17",
    })?;
    crate::register_actions(app);
    crate::runtime::register_runtime(app);
    Ok(Handles {})
}
