use nmp_core::substrate::{
    ActionRegistrar, ConfiguredRelaysChangeRegistrar, DmInboxRelayRegistrar, HostCapabilities,
    IdentityChangeRegistrar, IngestParserRegistrar, SnapshotProjectionRegistrar,
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
              + SnapshotProjectionRegistrar),
    _config: Config,
) -> Result<Handles, nmp_core::substrate::RegistrationError> {
    crate::register_actions(app);
    crate::runtime::register_runtime(app);
    Ok(Handles {})
}
