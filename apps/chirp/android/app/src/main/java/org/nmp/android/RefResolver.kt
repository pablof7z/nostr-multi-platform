package org.nmp.android

/**
 * ADR-0063 Lane D/G (#1671) — the unified `resolve_ref` parameter enums, the
 * Android twins of the iOS `RefNamespace` / `RefShape` / `RefLiveness`. The wire
 * codes are the contract the Rust resolver fails-closed on; they MUST match the
 * iOS enums and the `nmp_app_resolve_ref` C-ABI exactly.
 */

/** Resolver namespace. `0` = profile (kind:0), `1` = event. */
enum class RefNamespace(val code: Int) {
    Profile(0),
    Event(1),
}

/**
 * The requested projection shape for a `resolve_ref` claim. Codes are GLOBALLY
 * UNIQUE across namespaces (the kernel fails closed on a namespace/shape
 * mismatch); each shape is valid with exactly one namespace:
 *   * [ProfileRef]  (0) — `{pubkey, display_name, picture_url}` feed-avatar
 *     subset; namespace [RefNamespace.Profile]. Feed cards / lists / avatars.
 *   * [ProfileCard] (1) — full ProfileCard; namespace [RefNamespace.Profile].
 *     The open profile screen.
 *   * [EventEmbed]  (2) — render-an-embed-card subset; namespace
 *     [RefNamespace.Event].
 *   * [EventRaw]    (3) — full raw event; namespace [RefNamespace.Event].
 */
enum class RefShape(val code: Int) {
    ProfileRef(0),
    ProfileCard(1),
    EventEmbed(2),
    EventRaw(3),
}

/**
 * Liveness intent for a `resolve_ref` claim. [CacheOk] (0) serves from the store
 * with a OneShot fill and no live sub (feed rows / background); [Live] (1) keeps
 * a tailing sub open while the consumer holds the key (the open screen).
 */
enum class RefLiveness(val code: Int) {
    CacheOk(0),
    Live(1),
}
