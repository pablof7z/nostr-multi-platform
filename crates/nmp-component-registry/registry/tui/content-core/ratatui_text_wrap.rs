use ratatui::{
    style::Style,
    text::{Line, Span},
};

pub fn wrap_plain(value: &str, width: usize) -> Vec<Line<'static>> {
    wrap_words(value, width)
        .into_iter()
        .map(Line::from)
        .collect()
}

pub fn wrap_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from("")];
    }

    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut used = 0usize;

    for span in spans {
        let style = span.style;
        let text = span.content.to_string();
        let mut word = String::new();
        for ch in text.chars() {
            if ch == '\n' {
                push_piece(&mut lines, &mut current, &mut used, &word, style, width);
                word.clear();
                lines.push(line_from_spans(std::mem::take(&mut current)));
                used = 0;
            } else if ch.is_whitespace() {
                push_piece(&mut lines, &mut current, &mut used, &word, style, width);
                word.clear();
                if used > 0 && used < width {
                    current.push(Span::styled(" ".to_string(), style));
                    used += 1;
                }
            } else {
                word.push(ch);
            }
        }
        push_piece(&mut lines, &mut current, &mut used, &word, style, width);
    }

    if !current.is_empty() || lines.is_empty() {
        lines.push(line_from_spans(current));
    }
    lines
}

pub fn wrap_prefixed(
    value: &str,
    width: usize,
    prefix: &str,
    prefix_style: Style,
) -> Vec<Line<'static>> {
    let body_width = width.saturating_sub(prefix.chars().count()).max(1);
    wrap_words(value, body_width)
        .into_iter()
        .map(|line| {
            Line::from(vec![
                Span::styled(prefix.to_string(), prefix_style),
                Span::raw(line),
            ])
        })
        .collect()
}

fn push_piece(
    lines: &mut Vec<Line<'static>>,
    current: &mut Vec<Span<'static>>,
    used: &mut usize,
    piece: &str,
    style: Style,
    width: usize,
) {
    if piece.is_empty() {
        return;
    }
    for chunk in split_chars(piece, width) {
        let len = chunk.chars().count();
        if *used > 0 && *used + len > width {
            lines.push(line_from_spans(std::mem::take(current)));
            *used = 0;
        }
        current.push(Span::styled(chunk, style));
        *used += len;
    }
}

fn split_chars(value: &str, width: usize) -> Vec<String> {
    if value.chars().count() <= width {
        return vec![value.to_string()];
    }
    let mut out = Vec::new();
    let mut chunk = String::new();
    for ch in value.chars() {
        if chunk.chars().count() == width {
            out.push(std::mem::take(&mut chunk));
        }
        chunk.push(ch);
    }
    if !chunk.is_empty() {
        out.push(chunk);
    }
    out
}

fn line_from_spans(spans: Vec<Span<'static>>) -> Line<'static> {
    let out = spans
        .into_iter()
        .filter(|span| span.content != "\n")
        .collect::<Vec<_>>();
    Line::from(out)
}

fn wrap_words(value: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut line = String::new();
    for word in value.replace('\n', " ").split_whitespace() {
        let next = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if next.chars().count() > width && !line.is_empty() {
            out.push(std::mem::take(&mut line));
            line.push_str(word);
        } else {
            line = next;
        }
    }
    if !line.is_empty() || out.is_empty() {
        out.push(line);
    }
    out
}
