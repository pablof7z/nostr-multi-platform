import Foundation

/// Typed navigation destination for a user profile. Kept in its own enum so
/// we can later add `.npub(String)` / `.nip05(String)` cases without touching
/// every call site.
enum ProfileDestination: Hashable {
    case pubkey(String)
}
