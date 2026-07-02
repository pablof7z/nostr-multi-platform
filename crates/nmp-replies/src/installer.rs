use nmp_core::substrate::ActionRegistrar;

#[derive(Clone, Debug, Default)]
pub struct Config;

#[derive(Clone, Debug, Default)]
pub struct Handles;

pub fn register(
    app: &mut impl ActionRegistrar,
    _config: Config,
) -> Result<Handles, nmp_core::substrate::RegistrationError> {
    crate::action::register_actions(app);
    Ok(Handles)
}
