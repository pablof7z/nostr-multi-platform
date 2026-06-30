nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.app_gallery",
    crate_name: "nmp-app-gallery",
    summary: "NmpGallery explicit composition root over nmp-native-runtime and nmp-defaults. Produces libnmp_app_gallery for iOS and Android using ADR-0069 named installers.",
    claims: [
        {
            claim_type: "mechanism",
            id: "app.gallery.composition_root",
            exclusive: true,
            scope: {
                kind: "function",
                value: "register_gallery_app",
                context: "",
            },
            owns: [
                "Gallery app composition root over reusable NMP crates",
            ],
        },
    ],
    notes: [
    ],
}
