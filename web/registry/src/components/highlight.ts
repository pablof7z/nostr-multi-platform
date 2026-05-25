/*
 * Tiny dependency-free Swift highlighter.
 *
 * Produces an HTML string with <span class="tok-*"> wrappers for keywords,
 * strings, comments, numbers, and types. Designed for our subset — not a
 * complete Swift parser. ~50 lines beats shipping a 200KB library.
 *
 * Token order matters: we strip comments and strings first (they're
 * lexically dominant), then run keyword/type/number passes on the rest.
 */

const SWIFT_KEYWORDS = new Set([
  "import",
  "public",
  "private",
  "internal",
  "fileprivate",
  "open",
  "struct",
  "class",
  "enum",
  "protocol",
  "extension",
  "func",
  "var",
  "let",
  "init",
  "self",
  "Self",
  "return",
  "if",
  "else",
  "guard",
  "for",
  "in",
  "while",
  "do",
  "try",
  "throws",
  "throw",
  "catch",
  "switch",
  "case",
  "default",
  "break",
  "continue",
  "where",
  "as",
  "is",
  "nil",
  "true",
  "false",
  "static",
  "mutating",
  "inout",
  "associatedtype",
  "typealias",
  "indirect",
  "lazy",
  "weak",
  "unowned",
  "some",
  "any",
  "operator",
  "precedencegroup",
  "@escaping",
  "@discardableResult",
  "@MainActor",
  "@ViewBuilder",
  "@Environment",
  "@Binding",
  "@State",
  "@StateObject",
  "@ObservedObject",
  "@Published",
  "async",
  "await",
  "actor",
]);

const KNOWN_TYPES = new Set([
  "View",
  "Text",
  "Button",
  "Image",
  "VStack",
  "HStack",
  "ZStack",
  "ForEach",
  "List",
  "ScrollView",
  "NavigationStack",
  "NavigationLink",
  "URL",
  "String",
  "Int",
  "Double",
  "Float",
  "Bool",
  "Array",
  "Dictionary",
  "Set",
  "Optional",
  "Color",
  "Font",
  "EnvironmentValues",
  "EnvironmentKey",
  "Layout",
  "Subviews",
  "ProposedViewSize",
  "CGSize",
  "CGRect",
  "CGPoint",
  "CGFloat",
  "Identifiable",
  "Equatable",
  "Hashable",
  "Codable",
  "Decodable",
  "Encodable",
  "Void",
]);

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

const KOTLIN_KEYWORDS = new Set([
  "package",
  "import",
  "public",
  "private",
  "internal",
  "protected",
  "open",
  "final",
  "abstract",
  "sealed",
  "data",
  "object",
  "companion",
  "class",
  "interface",
  "enum",
  "fun",
  "val",
  "var",
  "lateinit",
  "const",
  "this",
  "super",
  "null",
  "true",
  "false",
  "return",
  "if",
  "else",
  "when",
  "for",
  "in",
  "is",
  "as",
  "while",
  "do",
  "try",
  "catch",
  "finally",
  "throw",
  "break",
  "continue",
  "by",
  "where",
  "out",
  "inline",
  "noinline",
  "crossinline",
  "reified",
  "operator",
  "infix",
  "tailrec",
  "vararg",
  "suspend",
  "typealias",
  "init",
  "constructor",
  "override",
  "annotation",
  "@Composable",
  "@Serializable",
  "@SerialName",
  "@JsonClassDiscriminator",
  "@OptIn",
  "@Suppress",
]);

/**
 * Highlight a Swift source string. Returns a safe HTML string.
 *
 * Strategy: scan character-by-character, emit one token at a time. This is
 * a single-pass lexer so we don't have to deal with placeholder-substitution
 * collisions between strings, comments, and keywords.
 */
export function highlightSwift(source: string): string {
  let out = "";
  let i = 0;
  const n = source.length;

  const peek = (offset = 0): string =>
    i + offset < n ? source[i + offset] : "";

  while (i < n) {
    const ch = source[i];

    // Line comment
    if (ch === "/" && peek(1) === "/") {
      let end = source.indexOf("\n", i);
      if (end === -1) end = n;
      out += `<span class="tok-com">${escapeHtml(source.slice(i, end))}</span>`;
      i = end;
      continue;
    }

    // Block comment
    if (ch === "/" && peek(1) === "*") {
      let end = source.indexOf("*/", i + 2);
      if (end === -1) end = n;
      else end += 2;
      out += `<span class="tok-com">${escapeHtml(source.slice(i, end))}</span>`;
      i = end;
      continue;
    }

    // String literal (handles escapes; no multi-line """ support — fine for our set)
    if (ch === '"') {
      let end = i + 1;
      while (end < n) {
        if (source[end] === "\\" && end + 1 < n) {
          end += 2;
          continue;
        }
        if (source[end] === '"') {
          end += 1;
          break;
        }
        if (source[end] === "\n") break;
        end += 1;
      }
      out += `<span class="tok-str">${escapeHtml(source.slice(i, end))}</span>`;
      i = end;
      continue;
    }

    // Number (decimal + simple float)
    if (/[0-9]/.test(ch)) {
      let end = i + 1;
      while (end < n && /[0-9_.]/.test(source[end])) end += 1;
      out += `<span class="tok-num">${escapeHtml(source.slice(i, end))}</span>`;
      i = end;
      continue;
    }

    // Attribute / identifier (also catches @-prefixed attributes)
    if (/[A-Za-z_@]/.test(ch)) {
      let end = i + 1;
      while (end < n && /[A-Za-z0-9_]/.test(source[end])) end += 1;
      const word = source.slice(i, end);

      if (SWIFT_KEYWORDS.has(word)) {
        out += `<span class="tok-kw">${escapeHtml(word)}</span>`;
      } else if (KNOWN_TYPES.has(word) || /^[A-Z][A-Za-z0-9_]*$/.test(word)) {
        out += `<span class="tok-type">${escapeHtml(word)}</span>`;
      } else {
        out += escapeHtml(word);
      }
      i = end;
      continue;
    }

    // Everything else (whitespace, punctuation, etc.)
    out += escapeHtml(ch);
    i += 1;
  }

  return out;
}

/**
 * Highlight a Kotlin source string. Shares the lexer with [highlightSwift] —
 * only the keyword set differs. Compose components annotated with `@Composable`
 * and `@Serializable` get the same `tok-kw` styling so the output is visually
 * consistent across the two targets.
 */
export function highlightKotlin(source: string): string {
  let out = "";
  let i = 0;
  const n = source.length;

  const peek = (offset = 0): string =>
    i + offset < n ? source[i + offset] : "";

  while (i < n) {
    const ch = source[i];

    if (ch === "/" && peek(1) === "/") {
      let end = source.indexOf("\n", i);
      if (end === -1) end = n;
      out += `<span class="tok-com">${escapeHtml(source.slice(i, end))}</span>`;
      i = end;
      continue;
    }

    if (ch === "/" && peek(1) === "*") {
      let end = source.indexOf("*/", i + 2);
      if (end === -1) end = n;
      else end += 2;
      out += `<span class="tok-com">${escapeHtml(source.slice(i, end))}</span>`;
      i = end;
      continue;
    }

    if (ch === '"') {
      let end = i + 1;
      while (end < n) {
        if (source[end] === "\\" && end + 1 < n) {
          end += 2;
          continue;
        }
        if (source[end] === '"') {
          end += 1;
          break;
        }
        if (source[end] === "\n") break;
        end += 1;
      }
      out += `<span class="tok-str">${escapeHtml(source.slice(i, end))}</span>`;
      i = end;
      continue;
    }

    if (/[0-9]/.test(ch)) {
      let end = i + 1;
      while (end < n && /[0-9_.fFL]/.test(source[end])) end += 1;
      out += `<span class="tok-num">${escapeHtml(source.slice(i, end))}</span>`;
      i = end;
      continue;
    }

    if (/[A-Za-z_@]/.test(ch)) {
      let end = i + 1;
      while (end < n && /[A-Za-z0-9_]/.test(source[end])) end += 1;
      const word = source.slice(i, end);

      if (KOTLIN_KEYWORDS.has(word)) {
        out += `<span class="tok-kw">${escapeHtml(word)}</span>`;
      } else if (/^[A-Z][A-Za-z0-9_]*$/.test(word)) {
        out += `<span class="tok-type">${escapeHtml(word)}</span>`;
      } else {
        out += escapeHtml(word);
      }
      i = end;
      continue;
    }

    out += escapeHtml(ch);
    i += 1;
  }

  return out;
}
