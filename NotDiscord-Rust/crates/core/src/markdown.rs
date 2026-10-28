//! Bounded, source-preserving message Markdown. No HTML, I/O or URL resolution.
//!
//! Every span indexes the original UTF-8 input. Syntax spans remain available to
//! an editor, while a message renderer can omit them without rewriting its model.
//! Limits stop interpretation, not preservation: an uninterpreted tail is plain.
use std::ops::Range;

const MAX_PARSE_BYTES: usize = 64 * 1024;
const MAX_BLOCKS: usize = 2048;
const MAX_SPANS: usize = 8192;
const MAX_DEPTH: usize = 16;
const MAX_WORK: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub code: bool,
    pub spoiler: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub style: Style,
    /// Delimiters/prefixes that an editor keeps but a formatted message hides.
    pub syntax: bool,
    /// Original destination range for a Markdown link's visible label.
    pub link: Option<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading { level: u8 },
    Subtext,
    ListItem { ordered: bool, number: u32, depth: u8 },
    CodeBlock { language: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// Includes prefixes and the original line ending/fences.
    pub range: Range<usize>,
    pub content: Range<usize>,
    pub kind: BlockKind,
    pub quote_depth: u8,
    /// Contiguous, nonoverlapping coverage of `range`.
    pub spans: Vec<Span>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub atoms: Vec<crate::message_atoms::Atom>,
    /// Some input was retained as plain text after an interpretation limit.
    pub truncated: bool,
}

struct Budget {
    remaining: usize,
    spans: usize,
    limited: bool,
}
impl Budget {
    fn work(&mut self, count: usize) -> bool {
        if count > self.remaining {
            self.limited = true;
            self.remaining = 0;
            false
        } else {
            self.remaining -= count;
            true
        }
    }
    fn available(&mut self) -> bool {
        if self.spans >= MAX_SPANS || self.remaining == 0 {
            self.limited = true;
            false
        } else { true }
    }
}

/// Interpret the supported Discord-style subset while preserving the source.
pub fn parse(source: &str) -> Document {
    let mut limit = source.len().min(MAX_PARSE_BYTES);
    while !source.is_char_boundary(limit) { limit -= 1; }
    let mut budget = Budget {
        remaining: source.len().saturating_mul(16).saturating_add(1024).min(MAX_WORK),
        spans: 0,
        limited: source.len() > limit,
    };
    let mut document = Document::default();
    let mut cursor = 0;
    let mut multiline_quote = 0;
    while cursor < limit && document.blocks.len() < MAX_BLOCKS && budget.available() {
        let (mut body_end, mut line_end) = line_limits(source, cursor, limit);
        let (mut content_start, quote_depth, indent) = prefix(source, cursor, body_end, &mut multiline_quote);
        if let Some((fence_end, fence_len)) = fence(source, content_start, body_end) {
            if let Some(close) = find_code_close(source, fence_end, limit, &source[content_start..fence_end], &mut budget) {
                let candidate = &source[fence_end..body_end];
                let language = (close >= line_end && line_end > body_end && !candidate.is_empty()
                    && candidate.len() <= 64 && candidate.bytes().all(|byte|
                        byte.is_ascii_alphanumeric() || b"_+-.#".contains(&byte)))
                    .then(|| candidate.to_owned());
                let mut code_start = if language.is_some() { line_end } else { fence_end };
                while code_start < close && matches!(source.as_bytes()[code_start], b'\r' | b'\n') { code_start += 1; }
                let mut code_end = close;
                while code_end > code_start && matches!(source.as_bytes()[code_end - 1], b'\r' | b'\n') { code_end -= 1; }
                if code_start < code_end {
                    let mut block_end = close + fence_len;
                    if source.as_bytes().get(block_end) == Some(&b'\r') && source.as_bytes().get(block_end + 1) == Some(&b'\n') {
                        block_end += 2;
                    } else if source.as_bytes().get(block_end) == Some(&b'\n') { block_end += 1; }
                    let mut spans = Vec::new();
                    let style = Style { code: true, ..Default::default() };
                    emit(&mut spans, cursor..code_start, style, true, None, &mut budget);
                    emit(&mut spans, code_start..code_end, style, false, None, &mut budget);
                    emit(&mut spans, code_end..block_end, style, true, None, &mut budget);
                    document.blocks.push(Block { range: cursor..block_end, content: code_start..code_end,
                        kind: BlockKind::CodeBlock { language }, quote_depth, spans });
                    cursor = block_end;
                    continue;
                }
            }
        }
        let mut kind = BlockKind::Paragraph;
        let mut base = Style::default();
        if source[content_start..body_end].starts_with("-# ") {
            content_start += 3;
            kind = BlockKind::Subtext;
        } else if let Some((end, level)) = heading(source, content_start, body_end) {
            content_start = end;
            kind = BlockKind::Heading { level };
            base.bold = true;
        } else if let Some((end, ordered, number)) = list_marker(source, content_start, body_end) {
            content_start = end;
            kind = BlockKind::ListItem { ordered, number, depth: (indent / 2).min(10) as u8 };
        }
        // Ordinary consecutive lines form one paragraph so emphasis can cross
        // an explicit line break. Structural prefixes and blank lines remain
        // independent blocks; every original line ending stays in the spans.
        if matches!(kind, BlockKind::Paragraph) && quote_depth == 0
            && !source[content_start..body_end].trim().is_empty() {
            while line_end < limit && budget.work(1) {
                let (next_body, next_end) = line_limits(source, line_end, limit);
                if source[line_end..next_body].trim().is_empty() { break; }
                let mut probe_quote = multiline_quote;
                let (next_start, next_quote, _) = prefix(source, line_end, next_body, &mut probe_quote);
                if next_quote != 0 || heading(source, next_start, next_body).is_some()
                    || list_marker(source, next_start, next_body).is_some()
                    || fence(source, next_start, next_body).is_some()
                    || source[next_start..next_body].starts_with("-# ") { break; }
                if !budget.work(next_end - line_end) { break; }
                body_end = next_body;
                line_end = next_end;
            }
        }
        let mut spans = Vec::new();
        emit(&mut spans, cursor..content_start, base, true, None, &mut budget);
        inline(source, content_start..body_end, base, None, 0, &mut spans, &mut budget);
        emit(&mut spans, body_end..line_end, base, false, None, &mut budget);
        document.blocks.push(Block { range: cursor..line_end, content: content_start..body_end,
            kind, quote_depth, spans });
        cursor = line_end;
    }
    if cursor < source.len() {
        budget.limited = true;
        document.blocks.push(Block { range: cursor..source.len(), content: cursor..source.len(),
            kind: BlockKind::Paragraph, quote_depth: multiline_quote,
            spans: vec![Span { range: cursor..source.len(), style: Style::default(), syntax: false, link: None }] });
    }
    document.truncated = budget.limited;
    document.atoms = crate::message_atoms::collect(source, &document);
    document
}

fn line_limits(source: &str, start: usize, limit: usize) -> (usize, usize) {
    let end = source[start..limit].find('\n').map_or(limit, |offset| start + offset + 1);
    let body = if source.as_bytes().get(end.wrapping_sub(1)) == Some(&b'\n') { end - 1 } else { end };
    let body = if body > start && source.as_bytes().get(body - 1) == Some(&b'\r') { body - 1 } else { body };
    (body, end)
}

fn prefix(source: &str, start: usize, end: usize, multiline: &mut u8) -> (usize, u8, usize) {
    let bytes = source.as_bytes();
    let mut cursor = start;
    let mut indent = 0;
    while cursor < end && matches!(bytes[cursor], b' ' | b'\t') {
        indent += if bytes[cursor] == b'\t' { 4 } else { 1 };
        cursor += 1;
    }
    // The rest-of-message quote does not recursively consume further quote
    // markers: those remain literal content inside the existing quote.
    if *multiline != 0 { return (start, *multiline, indent); }
    if source[cursor..end].starts_with(">>> ") {
        *multiline = 1;
        return (cursor + 4, 1, indent);
    }
    let mut depth = 0u8;
    if source[cursor..end].starts_with("> ") {
        depth = 1;
        cursor += 2;
    }
    // Preserve ordinary paragraph indentation. Only block prefixes consume it.
    if depth == 0 && !source[cursor..end].starts_with("-# ")
        && heading(source, cursor, end).is_none() && list_marker(source, cursor, end).is_none()
        && fence(source, cursor, end).is_none() { cursor = start; }
    (cursor, depth, indent)
}

fn heading(source: &str, start: usize, end: usize) -> Option<(usize, u8)> {
    let bytes = source.as_bytes();
    let count = bytes[start..end].iter().take_while(|byte| **byte == b'#').count();
    (count > 0 && count <= 3 && bytes.get(start + count) == Some(&b' '))
        .then_some((start + count + 1, count as u8))
}
fn list_marker(source: &str, start: usize, end: usize) -> Option<(usize, bool, u32)> {
    let bytes = source.as_bytes();
    if start + 1 < end && matches!(bytes[start], b'-' | b'*') && bytes[start + 1] == b' ' {
        return Some((start + 2, false, 0));
    }
    let digits = bytes[start..end].iter().take(10).take_while(|byte| byte.is_ascii_digit()).count();
    if digits == 0 || start + digits + 1 >= end || bytes[start + digits] != b'.' || bytes[start + digits + 1] != b' ' {
        return None;
    }
    let number = source[start..start + digits].parse::<u64>().ok()?.clamp(1, 1_000_000_000) as u32;
    Some((start + digits + 2, true, number))
}
fn fence(source: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let count = source.as_bytes()[start..end].iter().take_while(|byte| **byte == b'`').count();
    (count >= 3).then_some((start + count, count))
}

fn emit(spans: &mut Vec<Span>, range: Range<usize>, style: Style, syntax: bool,
    link: Option<Range<usize>>, budget: &mut Budget) {
    if range.is_empty() { return; }
    if let Some(previous) = spans.last_mut().filter(|previous|
        previous.range.end == range.start && previous.style == style && previous.syntax == syntax && previous.link == link) {
        previous.range.end = range.end;
    } else {
        budget.spans += 1;
        spans.push(Span { range, style, syntax, link });
    }
}

fn inline(source: &str, range: Range<usize>, style: Style, link: Option<Range<usize>>, depth: usize,
    spans: &mut Vec<Span>, budget: &mut Budget) {
    if depth >= MAX_DEPTH || !budget.available() {
        budget.limited |= depth >= MAX_DEPTH;
        emit(spans, range, style, false, link, budget);
        return;
    }
    let bytes = source.as_bytes();
    let mut cursor = range.start;
    let mut plain = cursor;
    while cursor < range.end && budget.available() && budget.work(1) {
        if bytes[cursor] == b'\\' && cursor + 1 < range.end && bytes[cursor + 1].is_ascii_punctuation() {
            emit(spans, plain..cursor, style, false, link.clone(), budget);
            emit(spans, cursor..cursor + 1, style, true, None, budget);
            emit(spans, cursor + 1..cursor + 2, style, false, link.clone(), budget);
            cursor += 2; plain = cursor; continue;
        }
        if let Some(end) = atom_end(source, cursor, range.end) {
            cursor = end;
            continue;
        }
        if bytes[cursor] == b'`' {
            let count = bytes[cursor..range.end].iter().take_while(|byte| **byte == b'`').count();
            let marker = &source[cursor..cursor + count];
            if let Some(close) = find_code_close(source, cursor + count, range.end, marker, budget) {
                emit(spans, plain..cursor, style, false, link.clone(), budget);
                let code = Style { code: true, ..style };
                emit(spans, cursor..cursor + count, code, true, None, budget);
                emit(spans, cursor + count..close, code, false, link.clone(), budget);
                emit(spans, close..close + count, code, true, None, budget);
                cursor = close + count; plain = cursor; continue;
            }
            cursor += count;
            continue;
        }
        if bytes[cursor] == b'[' && link.is_none() {
            if let Some((label_end, target, end)) = markdown_link(source, cursor, range.end, budget) {
                emit(spans, plain..cursor, style, false, None, budget);
                emit(spans, cursor..cursor + 1, style, true, None, budget);
                inline(source, cursor + 1..label_end, style, Some(target), depth + 1, spans, budget);
                emit(spans, label_end..end, style, true, None, budget);
                cursor = end; plain = cursor; continue;
            }
        }
        let mut matched = false;
        let marker_run = if matches!(bytes[cursor], b'*' | b'_' | b'~' | b'|') {
            bytes[cursor..range.end].iter().take_while(|byte| **byte == bytes[cursor]).count()
        } else { 0 };
        let marker_length = if matches!(bytes[cursor], b'*' | b'_') { marker_run.min(3) }
            else if marker_run >= 2 { 2 } else { 0 };
        for marker in ["***", "___", "**", "__", "~~", "||", "*", "_"] {
            if marker.len() != marker_length || !source[cursor..range.end].starts_with(marker)
                || !can_open(source, cursor, range.end, marker) { continue; }
            if let Some(close) = find_close(source, cursor + marker.len(), range.end, marker, budget) {
                emit(spans, plain..cursor, style, false, link.clone(), budget);
                let nested = apply_style(style, marker);
                emit(spans, cursor..cursor + marker.len(), nested, true, None, budget);
                inline(source, cursor + marker.len()..close, nested, link.clone(), depth + 1, spans, budget);
                emit(spans, close..close + marker.len(), nested, true, None, budget);
                cursor = close + marker.len(); plain = cursor; matched = true; break;
            }
        }
        if !matched {
            cursor += if marker_run > 0 { marker_run } else { source[cursor..range.end].chars().next().unwrap().len_utf8() };
        }
    }
    emit(spans, plain..range.end, style, false, link, budget);
}

fn apply_style(mut style: Style, marker: &str) -> Style {
    match marker {
        "***" => { style.bold = true; style.italic = true; },
        "___" => { style.underline = true; style.italic = true; },
        "**" => style.bold = true,
        "__" => style.underline = true,
        "~~" => style.strikethrough = true,
        "||" => style.spoiler = true,
        "*" | "_" => style.italic = true,
        _ => {},
    }
    style
}
fn can_open(source: &str, at: usize, end: usize, marker: &str) -> bool {
    let after = at + marker.len();
    let next = source[after..end].chars().next();
    if next.is_none_or(char::is_whitespace) { return false; }
    if marker.starts_with('_') && source[..at].chars().next_back().is_some_and(char::is_alphanumeric)
        && next.is_some_and(char::is_alphanumeric) { return false; }
    true
}
fn can_close(source: &str, start: usize, at: usize, end: usize, marker: &str) -> bool {
    if at <= start { return false; }
    let content = &source[start..at];
    let Some(previous) = content.chars().next_back() else { return false; };
    // The user's received-message example retains bold with a space before
    // the closing **. Extend only strong/combined-strong delimiters; a blank
    // payload and the existing single-emphasis flanking rules stay literal.
    if previous.is_whitespace()
        && !(matches!(marker, "**" | "***") && content.chars().any(|character| !character.is_whitespace())) {
        return false;
    }
    if marker.starts_with('_') && source[start..at].chars().next_back().is_some_and(char::is_alphanumeric)
        && source[at + marker.len()..end].chars().next().is_some_and(char::is_alphanumeric) { return false; }
    true
}
fn find_close(source: &str, start: usize, end: usize, marker: &str, budget: &mut Budget) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut cursor = start;
    while cursor < end && budget.work(1) {
        if bytes[cursor] == b'\\' && cursor + 1 < end {
            cursor += 1 + source[cursor + 1..end].chars().next()?.len_utf8();
            continue;
        }
        if let Some(atom) = atom_end(source, cursor, end) { cursor = atom; continue; }
        if bytes[cursor] == b'`' {
            let count = bytes[cursor..end].iter().take_while(|byte| **byte == b'`').count();
            if let Some(close) = find_code_close(source, cursor + count, end, &source[cursor..cursor + count], budget) {
                cursor = close + count; continue;
            }
            // Retrying every suffix of an unmatched run would be quadratic.
            cursor += count;
            continue;
        }
        if source[cursor..end].starts_with(marker) {
            let run = bytes[cursor..end].iter().take_while(|byte| **byte == marker.as_bytes()[0]).count();
            let candidate = cursor + run.saturating_sub(marker.len());
            if can_close(source, start, candidate, end, marker) {
                if !budget.work(candidate - start) { return None; }
                if source.as_bytes()[start..candidate].iter().any(|byte| *byte != marker.as_bytes()[0]) {
                    return Some(candidate);
                }
            }
            cursor += run.max(1);
        } else { cursor += source[cursor..end].chars().next()?.len_utf8(); }
    }
    None
}
fn find_code_close(source: &str, start: usize, end: usize, marker: &str, budget: &mut Budget) -> Option<usize> {
    let mut cursor = start;
    while cursor < end && budget.work(1) {
        if source[cursor..end].starts_with(marker) {
            let count = source.as_bytes()[cursor..end].iter().take_while(|byte| **byte == b'`').count();
            if count == marker.len() { return Some(cursor); }
            cursor += count;
        } else { cursor += source[cursor..end].chars().next()?.len_utf8(); }
    }
    None
}

/// Do not interpret emphasis inside Discord tokens or bare HTTP(S) URLs.
fn atom_end(source: &str, start: usize, end: usize) -> Option<usize> {
    let tail = &source[start..end];
    if ["<@", "<#", "<:", "<a:", "<t:"].iter().any(|prefix| tail.starts_with(prefix)) {
        let mut bound = tail.len().min(257);
        while !tail.is_char_boundary(bound) { bound -= 1; }
        if let Some(offset) = tail[..bound].find('>') {
            if !tail[..offset].chars().any(char::is_whitespace) { return Some(start + offset + 1); }
        }
    }
    let url_tail = tail.strip_prefix('<').unwrap_or(tail);
    if url_tail.get(..7).is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
        || url_tail.get(..8).is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://")) {
        let offset = tail.char_indices().find(|(_, ch)| ch.is_whitespace() || matches!(ch, '>' | '`'))
            .map_or(tail.len(), |(index, _)| index);
        let mut stop = start + offset;
        while stop > start && matches!(source.as_bytes()[stop - 1], b'*' | b'_' | b'~' | b'|') { stop -= 1; }
        if stop > start { return Some(stop); }
    }
    None
}

fn markdown_link(source: &str, start: usize, end: usize, budget: &mut Budget)
    -> Option<(usize, Range<usize>, usize)> {
    let bytes = source.as_bytes();
    let mut cursor = start + 1;
    let mut nested = 0;
    let label_end = loop {
        if cursor >= end || !budget.work(1) { return None; }
        match bytes[cursor] {
            b'\\' if cursor + 1 < end => { cursor += 1 + source[cursor + 1..end].chars().next()?.len_utf8(); continue; },
            b'[' => { nested += 1; if nested > 8 { return None; } },
            b']' if nested == 0 => break cursor,
            b']' => nested -= 1,
            _ => {},
        }
        cursor += source[cursor..end].chars().next()?.len_utf8();
    };
    if label_end == start + 1 || bytes.get(label_end + 1) != Some(&b'(') { return None; }
    cursor = label_end + 2;
    let target_start = cursor;
    nested = 0;
    while cursor < end && budget.work(1) {
        match bytes[cursor] {
            b'(' => { nested += 1; if nested > 8 { return None; } },
            b')' if nested == 0 => {
                let target = target_start..cursor;
                let value = &source[target.clone()];
                if value.len() > 2048 || value.chars().any(|ch| ch.is_whitespace() || ch.is_control()) { return None; }
                let valid = url::Url::parse(value).is_ok_and(|url| matches!(url.scheme(), "http" | "https")
                    && url.host_str().is_some() && url.username().is_empty() && url.password().is_none());
                return valid.then_some((label_end, target, cursor + 1));
            },
            b')' => nested -= 1,
            b'\n' | b'\r' => return None,
            _ => {},
        }
        cursor += source[cursor..end].chars().next()?.len_utf8();
    }
    None
}
