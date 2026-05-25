export type HighlightOptions = {
  keywords: Set<string>;
  identifierStart: RegExp;
  identifierContinue: RegExp;
  numberContinue: RegExp;
  isType?: (word: string) => boolean;
};

export function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

export function highlightCurlyLanguage(
  source: string,
  options: HighlightOptions,
): string {
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
      while (end < n && options.numberContinue.test(source[end])) end += 1;
      out += `<span class="tok-num">${escapeHtml(source.slice(i, end))}</span>`;
      i = end;
      continue;
    }

    if (options.identifierStart.test(ch)) {
      let end = i + 1;
      while (end < n && options.identifierContinue.test(source[end])) end += 1;
      const word = source.slice(i, end);

      if (options.keywords.has(word)) {
        out += `<span class="tok-kw">${escapeHtml(word)}</span>`;
      } else if (options.isType?.(word)) {
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
