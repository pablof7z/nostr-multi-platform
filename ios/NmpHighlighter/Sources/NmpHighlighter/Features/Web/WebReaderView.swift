import SwiftUI
@preconcurrency import WebKit

/// Full-page reader for web-URL highlights. Loads the target URL in a
/// `WKWebView`, runs Mozilla Readability against the loaded DOM, and — if
/// extraction yields meaningful content — replaces the page body with a
/// styled "reader mode" layout. Falls back to the raw page when readability
/// fails or the user toggles off.
///
/// After the page settles, a self-contained JS pass finds the highlighted
/// quote in the (possibly transformed) DOM and wraps it in a `<mark>` with
/// our accent color, then smooth-scrolls it into view.
struct WebReaderView: View {
    let target: WebReaderTarget

    @Environment(HighlighterStore.self) private var app
    @State private var isLoading: Bool = true
    @State private var loadProgress: Double = 0
    @State private var showAsReader: Bool = true
    @State private var readerAvailable: Bool = false
    @State private var shareTarget: ShareToCommunityTarget?
    @State private var sharePreparing = false
    @State private var shareError: String?

    var body: some View {
        ZStack(alignment: .top) {
            WebView(
                url: target.url,
                highlightQuote: target.highlightQuote,
                showAsReader: $showAsReader,
                isLoading: $isLoading,
                loadProgress: $loadProgress,
                readerAvailable: $readerAvailable
            )

            if isLoading {
                ProgressView(value: loadProgress, total: 1.0)
                    .progressViewStyle(.linear)
                    .tint(Color.highlighterAccent)
                    .transition(.opacity)
            }
        }
        .background(Color.highlighterPaper.ignoresSafeArea())
        .navigationTitle(target.url.host ?? "")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar(.hidden, for: .tabBar)
        .toolbar {
            if readerAvailable {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        showAsReader.toggle()
                    } label: {
                        Image(systemName: showAsReader ? "doc.plaintext" : "textformat")
                    }
                    .accessibilityLabel(showAsReader ? "Show original page" : "Show reader view")
                }
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    Task { await prepareShare() }
                } label: {
                    if sharePreparing {
                        ProgressView()
                    } else {
                        Image(systemName: "square.and.arrow.up")
                    }
                }
                .disabled(sharePreparing)
                .accessibilityLabel("Share to room")
            }
        }
        .sheet(item: $shareTarget) { target in
            ShareToCommunitySheet(target: target)
                .environment(app)
                .presentationDetents([.medium, .large])
        }
        .alert("Couldn't share", isPresented: Binding(
            get: { shareError != nil },
            set: { if !$0 { shareError = nil } }
        )) {
            Button("OK", role: .cancel) { shareError = nil }
        } message: {
            Text(shareError ?? "")
        }
        .commentsAttachment(
            artifact: .external(id: target.url.absoluteString, kind: 0)
        )
    }

    /// Build an `ArtifactPreview` from the URL via the Rust core (which
    /// fetches the page metadata) and hand it to the share sheet.
    /// Falls back to a bare URL-only preview if the fetch fails so
    /// the user can still share the link without a title.
    private func prepareShare() async {
        sharePreparing = true
        defer { sharePreparing = false }
        do {
            let preview = try await app.safeCore.buildPreviewFromUrl(target.url.absoluteString)
            await MainActor.run {
                shareTarget = ShareToCommunityTarget(
                    kind: .artifactShare(preview: preview),
                    displayTitle: preview.title.isEmpty ? (target.url.host ?? target.url.absoluteString) : preview.title,
                    displaySubtitle: preview.description,
                    imageURL: preview.image.isEmpty ? nil : URL(string: preview.image)
                )
            }
        } catch {
            await MainActor.run {
                shareError = "Couldn't build a preview: \(error.localizedDescription)"
            }
        }
    }
}

// MARK: - WKWebView wrapper

private struct WebView: UIViewRepresentable {
    let url: URL
    let highlightQuote: String
    @Binding var showAsReader: Bool
    @Binding var isLoading: Bool
    @Binding var loadProgress: Double
    @Binding var readerAvailable: Bool

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()

        // Install Readability.js as a user script so `window.Readability`
        // exists by the time `didFinish` fires. Scoped to main frame —
        // subframes (ads, embeds) don't need it.
        let controller = WKUserContentController()
        if let js = Self.loadReadabilitySource() {
            let userScript = WKUserScript(
                source: js,
                injectionTime: .atDocumentEnd,
                forMainFrameOnly: true
            )
            controller.addUserScript(userScript)
        }
        config.userContentController = controller

        let webView = WKWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = context.coordinator
        webView.allowsBackForwardNavigationGestures = true
        context.coordinator.webView = webView
        context.coordinator.showAsReaderBinding = $showAsReader
        context.coordinator.readerAvailableBinding = $readerAvailable

        let progressObservation = webView.observe(\.estimatedProgress, options: [.new]) { _, change in
            Task { @MainActor in
                loadProgress = change.newValue ?? 0
            }
        }
        context.coordinator.progressObservation = progressObservation

        webView.load(URLRequest(url: url))
        return webView
    }

    func updateUIView(_ uiView: WKWebView, context: Context) {
        context.coordinator.highlightQuote = highlightQuote
        if context.coordinator.lastAppliedMode != showAsReader {
            context.coordinator.lastAppliedMode = showAsReader
            // Only reload if we've already finished once; the initial load
            // will pick up the current mode on its first didFinish.
            if context.coordinator.hasFinishedInitialLoad {
                uiView.reload()
            }
        }
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(
            highlightQuote: highlightQuote,
            isLoadingBinding: $isLoading
        )
    }

    private static func loadReadabilitySource() -> String? {
        guard let url = Bundle.main.url(forResource: "Readability", withExtension: "js") else {
            return nil
        }
        return try? String(contentsOf: url, encoding: .utf8)
    }

    @MainActor
    final class Coordinator: NSObject, WKNavigationDelegate {
        var highlightQuote: String
        let isLoadingBinding: Binding<Bool>
        var showAsReaderBinding: Binding<Bool>?
        var readerAvailableBinding: Binding<Bool>?
        weak var webView: WKWebView?
        var progressObservation: NSKeyValueObservation?
        var lastAppliedMode: Bool = true
        var hasFinishedInitialLoad: Bool = false

        init(highlightQuote: String, isLoadingBinding: Binding<Bool>) {
            self.highlightQuote = highlightQuote
            self.isLoadingBinding = isLoadingBinding
        }

        nonisolated func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
            Task { @MainActor in
                self.isLoadingBinding.wrappedValue = true
            }
        }

        nonisolated func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            Task { @MainActor in
                try? await Task.sleep(nanoseconds: 350_000_000)
                await self.applyMode()
                try? await Task.sleep(nanoseconds: 120_000_000)
                self.injectHighlight()
                self.isLoadingBinding.wrappedValue = false
                self.hasFinishedInitialLoad = true
            }
        }

        nonisolated func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
            Task { @MainActor in
                self.isLoadingBinding.wrappedValue = false
            }
        }

        nonisolated func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
            Task { @MainActor in
                self.isLoadingBinding.wrappedValue = false
            }
        }

        /// Reader-or-raw dispatch. Always probes Readability so the toolbar
        /// can surface the toggle when extraction is viable.
        private func applyMode() async {
            guard let webView else { return }
            let reader = showAsReaderBinding?.wrappedValue ?? true

            if reader {
                let js = Self.readerTransformScript()
                let result = try? await webView.evaluateJavaScript(js)
                let status = (result as? String) ?? ""
                readerAvailableBinding?.wrappedValue = (status == "ready" || status == "unavailable")
                    // `unavailable` still means the page was analyzable — we just
                    // didn't have enough content. But we only show the toggle
                    // when the reader *worked*, not when it didn't.
                    ? (status == "ready")
                    : false
            } else {
                // Raw mode — probe only, so the toggle stays visible.
                let probe = Self.readerProbeScript()
                let result = try? await webView.evaluateJavaScript(probe)
                let status = (result as? String) ?? ""
                readerAvailableBinding?.wrappedValue = (status == "ok")
            }
        }

        private func injectHighlight() {
            guard !highlightQuote.isEmpty, let webView else { return }
            let js = Self.buildHighlightScript(quote: highlightQuote)
            webView.evaluateJavaScript(js, completionHandler: nil)
        }

        // MARK: - Scripts

        /// Runs Readability against a document clone; if it yields usable
        /// content, mutates the live document to show a styled reader view.
        /// Returns `"ready"` / `"unavailable"` / `"error:<msg>"`.
        private static func readerTransformScript() -> String {
            return """
            (function() {
              try {
                if (typeof Readability !== 'function') return 'error:no-readability';
                var clone = document.cloneNode(true);
                var article = new Readability(clone).parse();
                if (!article || !article.content) return 'unavailable';
                var body = article.content;
                if (body.replace(/<[^>]+>/g, '').trim().length < 400) return 'unavailable';

                var escapeHtml = function(s) {
                  return String(s)
                    .replace(/&/g, '&amp;')
                    .replace(/</g, '&lt;')
                    .replace(/>/g, '&gt;')
                    .replace(/"/g, '&quot;')
                    .replace(/'/g, '&#39;');
                };

                var title = article.title || '';
                var byline = article.byline || article.siteName || '';

                var css = `
                  :root { color-scheme: light dark; }
                  html, body { margin: 0; padding: 0; background: #fafaf7; color: #15130f; }
                  @media (prefers-color-scheme: dark) {
                    html, body { background: #151310; color: #f3f0e9; }
                    .reader-byline { color: #ada69a !important; }
                    .reader-article a { color: #e09a78 !important; }
                    .reader-rule { border-color: #38332a !important; }
                  }
                  body { font: 18px/1.65 'Iowan Old Style', 'Palatino', Georgia, serif; }
                  .reader-article { max-width: 680px; margin: 0 auto; padding: 34px 22px 60px; }
                  .reader-title { font: 700 30px/1.2 -apple-system, BlinkMacSystemFont, 'SF Pro Text', sans-serif; margin: 0 0 10px; }
                  .reader-byline { font: 14px/1.4 -apple-system, BlinkMacSystemFont, 'SF Pro Text', sans-serif; color: #7a7468; margin: 0 0 6px; }
                  .reader-rule { border: 0; border-top: 1px solid #e5ddc9; margin: 22px 0 26px; }
                  .reader-body p { margin: 0 0 1em; }
                  .reader-body h1, .reader-body h2, .reader-body h3 { font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Text', sans-serif; }
                  .reader-body img, .reader-body figure { max-width: 100%; height: auto; display: block; margin: 1.2em auto; }
                  .reader-body figure figcaption { font: 13px/1.4 -apple-system, sans-serif; color: #7a7468; text-align: center; margin-top: 4px; }
                  .reader-body a { color: #c57d5f; text-decoration: underline; text-decoration-color: rgba(197,125,95,0.35); text-underline-offset: 2px; }
                  .reader-body pre, .reader-body code { font-family: 'SF Mono', Menlo, monospace; font-size: 0.9em; }
                  .reader-body pre { background: rgba(127,127,127,0.08); padding: 12px 14px; border-radius: 6px; overflow-x: auto; }
                  .reader-body blockquote { margin: 1em 0 1em 0; padding: 0 0 0 16px; border-left: 3px solid #c57d5f; color: #5a5448; font-style: italic; }
                  mark[data-highlighter] { background: rgba(197,125,95,0.32); color: inherit; padding: 0.05em 0.15em; border-radius: 2px; box-shadow: inset 0 -1px 0 rgba(197,125,95,0.6); }
                `;

                // Replace <head> with our own minimal head (strips page CSS/JS).
                document.head.innerHTML = '<meta name="viewport" content="width=device-width, initial-scale=1"><style>' + css + '</style>';

                var header = '<h1 class="reader-title">' + escapeHtml(title) + '</h1>';
                if (byline) {
                  header += '<p class="reader-byline">' + escapeHtml(byline) + '</p>';
                }
                header += '<hr class="reader-rule">';

                document.body.className = '';
                document.body.setAttribute('style', '');
                document.body.innerHTML =
                  '<article class="reader-article">' +
                    header +
                    '<div class="reader-body">' + body + '</div>' +
                  '</article>';

                // Kill any leftover inline styles on the <html> root from the
                // original page (some sites pin the background via style attr).
                document.documentElement.setAttribute('style', '');

                return 'ready';
              } catch (e) {
                return 'error:' + String(e && e.message || e);
              }
            })();
            """
        }

        /// Returns `'ok'` if Readability would yield readable content; never
        /// mutates the DOM. Used when the user chose "raw" mode so we can
        /// keep the toggle visible.
        private static func readerProbeScript() -> String {
            return """
            (function() {
              try {
                if (typeof Readability !== 'function') return 'no-readability';
                var a = new Readability(document.cloneNode(true)).parse();
                if (!a || !a.content) return 'no-content';
                return (a.content.replace(/<[^>]+>/g, '').trim().length >= 400) ? 'ok' : 'short';
              } catch (e) {
                return 'error:' + String(e && e.message || e);
              }
            })();
            """
        }

        /// Builds a self-contained IIFE that:
        ///  1. Normalizes whitespace in the needle + page text.
        ///  2. Walks text nodes, concatenates with a single-space joiner
        ///     while recording per-node (startOffset, endOffset) spans over
        ///     the normalized string.
        ///  3. Finds the needle in the normalized string, maps start/end
        ///     back to (node, offset) pairs, creates a DOM Range, wraps it
        ///     with a styled `<mark>`, and scrolls the mark into view.
        ///
        /// Handles quotes that span multiple text nodes and elements. Works
        /// in both the original page and the reader-mode DOM.
        private static func buildHighlightScript(quote: String) -> String {
            let encoded = try? JSONEncoder().encode(quote)
            let needleLiteral = encoded.flatMap { String(data: $0, encoding: .utf8) } ?? "\"\""
            return """
            (function() {
              try {
                var needleRaw = \(needleLiteral);
                if (!needleRaw) return;
                var normWs = function(s) { return s.replace(/\\s+/g, ' ').trim(); };
                var needle = normWs(needleRaw);
                if (needle.length < 4) return;

                var root = document.body;
                if (!root) return;

                var walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
                  acceptNode: function(n) {
                    var p = n.parentNode;
                    if (!p) return NodeFilter.FILTER_REJECT;
                    var tag = (p.tagName || '').toLowerCase();
                    if (tag === 'script' || tag === 'style' || tag === 'noscript' || tag === 'iframe') {
                      return NodeFilter.FILTER_REJECT;
                    }
                    return NodeFilter.FILTER_ACCEPT;
                  }
                });

                var parts = [];
                var norm = '';
                var node;
                while ((node = walker.nextNode())) {
                  var raw = node.nodeValue;
                  if (!raw) continue;
                  var nodeSpan = [];
                  var i = 0;
                  while (i < raw.length) {
                    var c = raw.charCodeAt(i);
                    if (c === 32 || c === 9 || c === 10 || c === 13 || c === 12 || c === 160) {
                      var runStart = i;
                      while (i < raw.length) {
                        var cc = raw.charCodeAt(i);
                        if (cc === 32 || cc === 9 || cc === 10 || cc === 13 || cc === 12 || cc === 160) { i++; }
                        else { break; }
                      }
                      if (norm.length > 0 && norm.charAt(norm.length - 1) !== ' ') {
                        nodeSpan.push({normStart: norm.length, origStart: runStart, origEnd: i});
                        norm += ' ';
                      }
                    } else {
                      nodeSpan.push({normStart: norm.length, origStart: i, origEnd: i + 1});
                      norm += raw.charAt(i);
                      i++;
                    }
                  }
                  parts.push({node: node, spans: nodeSpan});
                }

                var idx = norm.indexOf(needle);
                if (idx < 0) {
                  var probe = needle.slice(0, Math.min(80, needle.length));
                  if (probe.length < 12) return;
                  idx = norm.indexOf(probe);
                  if (idx < 0) return;
                  needle = probe;
                }
                var endIdx = idx + needle.length;

                function locate(target) {
                  for (var p = 0; p < parts.length; p++) {
                    var spans = parts[p].spans;
                    for (var s = 0; s < spans.length; s++) {
                      var span = spans[s];
                      if (span.normStart >= target) {
                        return {node: parts[p].node, offset: span.origStart};
                      }
                    }
                  }
                  if (parts.length === 0) return null;
                  var last = parts[parts.length - 1];
                  var lastSpan = last.spans[last.spans.length - 1];
                  return {node: last.node, offset: lastSpan ? lastSpan.origEnd : 0};
                }

                var startLoc = locate(idx);
                var endLoc = locate(endIdx);
                if (!startLoc || !endLoc) return;

                var range = document.createRange();
                range.setStart(startLoc.node, Math.min(startLoc.offset, startLoc.node.nodeValue.length));
                range.setEnd(endLoc.node, Math.min(endLoc.offset, endLoc.node.nodeValue.length));

                var mark = document.createElement('mark');
                mark.setAttribute('data-highlighter', '1');
                mark.style.backgroundColor = 'rgba(197, 125, 95, 0.32)';
                mark.style.color = 'inherit';
                mark.style.padding = '0.05em 0.15em';
                mark.style.borderRadius = '2px';
                mark.style.boxShadow = 'inset 0 -1px 0 rgba(197, 125, 95, 0.6)';

                try {
                  range.surroundContents(mark);
                } catch(e) {
                  try {
                    var frag = range.extractContents();
                    mark.appendChild(frag);
                    range.insertNode(mark);
                  } catch(e2) {
                    return;
                  }
                }

                setTimeout(function() {
                  try { mark.scrollIntoView({behavior: 'smooth', block: 'center'}); } catch(_) {}
                }, 80);
              } catch (err) {}
            })();
            """
        }
    }
}
