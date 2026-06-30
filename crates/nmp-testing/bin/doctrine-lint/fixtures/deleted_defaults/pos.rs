pub fn compose(app: &mut impl AppHost) {
    nmp_defaults::register_defaults_with_handles(app);
    let _preset = TestDefaults::new();
}
