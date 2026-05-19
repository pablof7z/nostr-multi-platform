import Foundation

/// Navigation payload for opening an external web source behind a highlight.
/// The iOS `NavigationStack` dispatches on the static type, so keep this
/// struct narrow — one page at a URL, optionally anchored on a quote we
/// should surface in the web page after it loads.
struct WebReaderTarget: Hashable {
    let url: URL
    /// The exact text that was highlighted — we inject JS after load to find
    /// and visually mark it, then scroll it into view. Empty string means
    /// "no highlight, just load the page."
    let highlightQuote: String
}
