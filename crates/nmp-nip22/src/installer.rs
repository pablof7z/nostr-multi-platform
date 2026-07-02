use std::sync::Arc;

use nmp_core::substrate::ObservedProjectionRegistrar;

use crate::CommentThreadProjection;

#[derive(Clone, Debug, Default)]
pub struct Config;

#[derive(Clone)]
pub struct Handles {
    pub comments: Arc<CommentThreadProjection>,
}

impl std::fmt::Debug for Handles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handles")
            .field("comments", &"CommentThreadProjection")
            .finish()
    }
}

pub fn register(
    app: &mut impl ObservedProjectionRegistrar,
    _config: Config,
) -> Result<Handles, nmp_core::substrate::RegistrationError> {
    let comments = crate::runtime::register_runtime(app);
    Ok(Handles { comments })
}
