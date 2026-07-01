//! `nmp export jsrepo [--output DIR]` command adapter.
//!
//! The component registry crate owns the registry manifests, source assets,
//! and jsrepo export model. The CLI owns only command dispatch and UX.

pub fn run(args: &[String]) -> Result<(), String> {
    nmp_component_registry::export::run(args)
}
