import Foundation

/// One section in the gallery sidebar (e.g. "User", "Content"). Groups a set
/// of `RegistryComponent` rows that share a domain.
struct RegistrySection: Identifiable, Hashable {
    let id: String
    let label: String
    let components: [RegistryComponent]
}

/// One component row inside a section. `id` doubles as the dispatch key the
/// detail view uses to pick the right page builder.
struct RegistryComponent: Identifiable, Hashable {
    /// Stable registry slug (e.g. `"user-avatar"`). MUST match the slugs
    /// `crates/nmp-cli/registry/swiftui/` uses on disk.
    let id: String
    /// Display label — the public Swift type name the component exports.
    let label: String
    /// Short, single-sentence description shown under `label` in the list.
    let description: String
}

/// Authoritative catalog of gallery components. Driven by the SwiftUI
/// registry layout under `crates/nmp-cli/registry/swiftui/`.
let REGISTRY_SECTIONS: [RegistrySection] = [
    RegistrySection(id: "relay", label: "Relay", components: [
        RegistryComponent(
            id: "relay-list",
            label: "NostrRelayList",
            description: "Relay URLs with role badges and connection status dots"),
    ]),
    RegistrySection(id: "user", label: "User", components: [
        RegistryComponent(
            id: "user-avatar",
            label: "NostrAvatar",
            description: "Circular avatar with identicon fallback"),
        RegistryComponent(
            id: "user-name",
            label: "NostrProfileName",
            description: "Display name with npub fallback"),
        RegistryComponent(
            id: "user-nip05",
            label: "NostrNip05Badge",
            description: "NIP-05 verified identity badge"),
        RegistryComponent(
            id: "user-npub",
            label: "NostrNpubChip",
            description: "Copyable npub chip"),
        RegistryComponent(
            id: "user-card",
            label: "NostrUserCard",
            description: "Compact avatar + name + nip05 row"),
    ]),
    RegistrySection(id: "content", label: "Content", components: [
        RegistryComponent(
            id: "content-core",
            label: "ContentTreeWire",
            description: "Wire type + identicon renderer"),
        RegistryComponent(
            id: "content-view",
            label: "NostrContentView",
            description: "Full rich content renderer"),
        RegistryComponent(
            id: "content-mention-chip",
            label: "NostrMentionChip",
            description: "Tappable @mention chip"),
        RegistryComponent(
            id: "content-minimal",
            label: "NostrMinimalContentView",
            description: "Flow-layout minimal renderer"),
        RegistryComponent(
            id: "content-media-grid",
            label: "NostrMediaGrid",
            description: "Photo-style image grid"),
        RegistryComponent(
            id: "content-quote-card",
            label: "NostrQuoteCard",
            description: "Embedded event quote card"),
    ]),
    RegistrySection(id: "embeds", label: "Embeds & Kinds", components: [
        RegistryComponent(
            id: "embed-article",
            label: "ArticleEmbed",
            description: "Kind:30023 long-form article — hero image, title, summary"),
        RegistryComponent(
            id: "embed-profile",
            label: "ProfileEmbed",
            description: "Inline npub mention chip — kind:0 profile"),
        RegistryComponent(
            id: "embed-note",
            label: "NoteEmbed",
            description: "Kind:1 short text note via nevent claim"),
        RegistryComponent(
            id: "embed-highlight",
            label: "HighlightEmbed",
            description: "Kind:9802 highlight — pull-quote + source"),
    ]),
    RegistrySection(id: "auth", label: "Auth", components: [
        RegistryComponent(
            id: "login-block",
            label: "NostrLoginBlock",
            description: "Signer detection + manual key-entry login UI"),
    ]),
]
