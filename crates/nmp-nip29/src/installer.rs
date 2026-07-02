use nmp_core::substrate::{
    ActionRegistrar, InputScopeRegistrar, RegistrationError, SearchScopeRegistrar,
};

#[derive(Clone, Debug, Default)]
pub struct Config;

#[derive(Clone, Debug, Default)]
pub struct Handles;

pub fn register(
    app: &mut (impl ActionRegistrar + InputScopeRegistrar + SearchScopeRegistrar),
    _config: Config,
) -> Result<Handles, RegistrationError> {
    crate::action_registration::register_actions(app)?;
    crate::input_scope::register_input_scopes(app);
    crate::search::register_search_scopes(app);
    Ok(Handles)
}
