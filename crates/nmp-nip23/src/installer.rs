use nmp_core::substrate::{ObservedProjectionRegistrar, SnapshotProjectionRegistrar};

#[derive(Clone, Debug, Default)]
pub struct Config {}

#[derive(Clone, Debug, Default)]
pub struct Handles {}

pub fn register(
    app: &(impl ObservedProjectionRegistrar + SnapshotProjectionRegistrar),
    _config: Config,
) -> Result<Handles, nmp_core::substrate::RegistrationError> {
    crate::register_longform_projection(app);
    Ok(Handles {})
}
