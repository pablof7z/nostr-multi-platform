pub fn compose(app: &mut impl AppHost) {
    register_router(app);
    register_mailbox_cache(app);
    register_profile_projection(app);
}
