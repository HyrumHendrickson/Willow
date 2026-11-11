//! Lightweight text markup used inside text boxes.
//!
//! * A line starting with `- `, `* ` or `• ` is a bullet. Two leading spaces (or one tab)
//!   per level nest the bullet, up to three levels deep.
//! * Text between `{{` and `}}` uses the text box's secondary style (its own font, size,
//!   colour, weight and slant). A box therefore holds two styles at once.
//!
//! The same parser feeds the editor preview and the HTML exporter, so both agree.

pub const MAX_LEVEL: u8 = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    /// Rendered with the secondary style.
    pub alt: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub bullet: bool,
    pub level: u8,
    pub spans: Vec<Span>,
}

impl Line {
    pub fn is_blank(&self) -> bool {
        self.spans.iter().all(|s| s.text.is_empty())
    }
}

/// Length in chars of the bullet marker (`- `) and the indentation before it.
fn marker(line: &[char]) -> Option<(usize, usize)> {
    let indent = line.iter().take_while(|c| **c == ' ' || **c == '\t').count();
    match (line.get(indent), line.get(indent + 1)) {
        (Some('-' | '*' | '•'), Some(' ')) => Some((indent, 2)),
        _ => None,
    }
}

fn level_of(line: &[char], indent: usize) -> u8 {
    let width: usize = line[..indent].iter().map(|c| if *c == '\t' { 2 } else { 1 }).sum();
    ((width / 2) as u8).min(MAX_LEVEL)
}

fn split_spans(content: &str, alt: &mut bool) -> Vec<Span> {
    let b = content.as_bytes();
    let mut spans = Vec::new();
    let mut push = |text: &str, alt: bool| {
        if !text.is_empty() {
            spans.push(Span { text: text.to_string(), alt });
        }
    };
    let (mut start, mut i) = (0, 0);
    while i + 1 < b.len() {
        if b[i] == b'{' && b[i + 1] == b'{' {
            push(&content[start..i], *alt);
            *alt = true;
            i += 2;
            start = i;
        } else if b[i] == b'}' && b[i + 1] == b'}' && *alt {
            push(&content[start..i], *alt);
            *alt = false;
            i += 2;
            start = i;
        } else {
            i += 1;
        }
    }
    push(&content[start..], *alt);
    spans
}

pub fn parse(text: &str) -> Vec<Line> {
    let mut alt = false;
    text.split('\n')
        .map(|raw| {
            let raw = raw.strip_suffix('\r').unwrap_or(raw);
            let chars: Vec<char> = raw.chars().collect();
            match marker(&chars) {
                Some((indent, len)) => {
                    let content: String = chars[indent + len..].iter().collect();
                    Line { bullet: true, level: level_of(&chars, indent), spans: split_spans(&content, &mut alt) }
                }
                None => Line { bullet: false, level: 0, spans: split_spans(raw, &mut alt) },
            }
        })
        .collect()
}

pub fn has_alt(text: &str) -> bool {
    text.contains("{{")
}

// ---------------------------------------------------------------- editing

fn clamp_range(len: usize, a: usize, b: usize) -> (usize, usize) {
    let (a, b) = (a.min(len), b.min(len));
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

enum Op {
    Insert(usize),
    Remove(usize),
}

fn remap(x: usize, ops: &[Op]) -> usize {
    let mut v = x as isize;
    for op in ops {
        match *op {
            Op::Insert(pos) if x >= pos => v += 2,
            Op::Remove(pos) if x >= pos + 2 => v -= 2,
            Op::Remove(pos) if x > pos => v -= (x - pos) as isize,
            _ => {}
        }
    }
    v.max(0) as usize
}

/// Adds or removes bullet markers on every line touched by the selection `[a, b]`
/// (char indices). Returns the new text and the adjusted selection.
pub fn toggle_bullets(text: &str, a: usize, b: usize) -> (String, usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let (a, b) = clamp_range(chars.len(), a, b);
    let lines: Vec<&[char]> = chars.split(|c| *c == '\n').collect();

    // (start, line) for every selected line
    let mut starts = Vec::with_capacity(lines.len());
    let mut at = 0;
    for l in &lines {
        starts.push(at);
        at += l.len() + 1;
    }
    let selected: Vec<usize> = (0..lines.len())
        .filter(|&i| {
            let (s, e) = (starts[i], starts[i] + lines[i].len());
            s <= b && e >= a && !(a < b && s == b)
        })
        .collect();

    let nonblank: Vec<usize> =
        selected.iter().copied().filter(|&i| lines[i].iter().any(|c| !c.is_whitespace())).collect();
    let remove = !nonblank.is_empty() && nonblank.iter().all(|&i| marker(lines[i]).is_some());

    let mut ops = Vec::new();
    for &i in &selected {
        let blank = !lines[i].iter().any(|c| !c.is_whitespace());
        match marker(lines[i]) {
            Some((indent, _)) if remove => ops.push(Op::Remove(starts[i] + indent)),
            None if !remove && (!blank || selected.len() == 1) => {
                let indent = lines[i].iter().take_while(|c| **c == ' ' || **c == '\t').count();
                ops.push(Op::Insert(starts[i] + indent));
            }
            _ => {}
        }
    }

    let mut out: Vec<char> = Vec::with_capacity(chars.len() + ops.len() * 2);
    let mut cursor = 0;
    for op in &ops {
        match *op {
            Op::Insert(pos) => {
                out.extend_from_slice(&chars[cursor..pos]);
                out.extend(['-', ' ']);
                cursor = pos;
            }
            Op::Remove(pos) => {
                out.extend_from_slice(&chars[cursor..pos]);
                cursor = pos + 2;
            }
        }
    }
    out.extend_from_slice(&chars[cursor..]);
    (out.into_iter().collect(), remap(a, &ops), remap(b, &ops))
}

/// Wraps the selection in `{{ }}` (secondary style), or unwraps it if already wrapped.
/// With no selection an empty pair is inserted and the caret placed between the markers.
pub fn toggle_alt(text: &str, a: usize, b: usize) -> (String, usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let (a, b) = clamp_range(chars.len(), a, b);
    let s = |from: usize, to: usize| -> String { chars[from..to].iter().collect() };

    if a == b {
        let out = format!("{}{{{{}}}}{}", s(0, a), s(a, chars.len()));
        return (out, a + 2, a + 2);
    }
    // markers just outside the selection
    if a >= 2 && b + 2 <= chars.len() && s(a - 2, a) == "{{" && s(b, b + 2) == "}}" {
        let out = format!("{}{}{}", s(0, a - 2), s(a, b), s(b + 2, chars.len()));
        return (out, a - 2, b - 2);
    }
    // markers inside the selection
    if b - a >= 4 && s(a, a + 2) == "{{" && s(b - 2, b) == "}}" {
        let out = format!("{}{}{}", s(0, a), s(a + 2, b - 2), s(b, chars.len()));
        return (out, a, b - 4);
    }
    let out = format!("{}{{{{{}}}}}{}", s(0, a), s(a, b), s(b, chars.len()));
    (out, a + 2, b + 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[Line]) -> Vec<(bool, u8, String)> {
        lines.iter().map(|l| (l.bullet, l.level, l.spans.iter().map(|s| s.text.as_str()).collect())).collect()
    }

    #[test]
    fn parses_bullets_and_levels() {
        let l = parse("Intro\n- one\n  * two\n    • three\n      - four\n-not a bullet");
        assert_eq!(
            plain(&l),
            vec![
                (false, 0, "Intro".to_string()),
                (true, 0, "one".to_string()),
                (true, 1, "two".to_string()),
                (true, 2, "three".to_string()),
                (true, 3, "four".to_string()),
                (false, 0, "-not a bullet".to_string()),
            ]
        );
        assert!(!parse("a - b\n-c").iter().any(|l| l.bullet));
    }

    #[test]
    fn parses_alt_spans_across_lines() {
        let l = parse("Hello {{small}} world\n{{two\nlines}} end");
        assert_eq!(
            l[0].spans,
            vec![
                Span { text: "Hello ".into(), alt: false },
                Span { text: "small".into(), alt: true },
                Span { text: " world".into(), alt: false },
            ]
        );
        assert_eq!(l[1].spans, vec![Span { text: "two".into(), alt: true }]);
        assert_eq!(
            l[2].spans,
            vec![Span { text: "lines".into(), alt: true }, Span { text: " end".into(), alt: false }]
        );
        // a stray closing marker is literal text
        assert_eq!(parse("a }} b")[0].spans, vec![Span { text: "a }} b".into(), alt: false }]);
        // unclosed marker styles the rest
        assert!(parse("x {{y")[0].spans[1].alt);
    }

    #[test]
    fn toggles_bullets() {
        let (t, a, b) = toggle_bullets("one\ntwo\nthree", 0, 7);
        assert_eq!(t, "- one\n- two\nthree");
        assert_eq!((a, b), (2, 11));
        let (t2, a2, b2) = toggle_bullets(&t, a, b);
        assert_eq!((t2.as_str(), a2, b2), ("one\ntwo\nthree", 0, 7));

        // caret only touches its own line; mixed selections add to the rest
        let (t, a, _) = toggle_bullets("a\n- b\nc", 4, 4);
        assert_eq!(t, "a\nb\nc");
        assert_eq!(a, 2);
        let (t, _, _) = toggle_bullets("a\n- b\nc", 0, 7);
        assert_eq!(t, "- a\n- b\n- c");

        // indentation is preserved
        let (t, _, _) = toggle_bullets("  nested", 0, 0);
        assert_eq!(t, "  - nested");
        // a caret on an empty line starts a bullet
        assert_eq!(toggle_bullets("", 0, 0), ("- ".to_string(), 2, 2));
        // unicode safe
        let (t, a, b) = toggle_bullets("héllo\nwörld", 0, 11);
        assert_eq!(t, "- héllo\n- wörld");
        assert_eq!((a, b), (2, 15));
    }

    #[test]
    fn toggles_alt() {
        assert_eq!(toggle_alt("abc def", 4, 7), ("abc {{def}}".to_string(), 6, 9));
        assert_eq!(toggle_alt("abc {{def}}", 6, 9), ("abc def".to_string(), 4, 7));
        assert_eq!(toggle_alt("abc {{def}}", 4, 11), ("abc def".to_string(), 4, 7));
        assert_eq!(toggle_alt("ab", 1, 1), ("a{{}}b".to_string(), 3, 3));
        assert_eq!(toggle_alt("héllo", 1, 3), ("h{{él}}lo".to_string(), 3, 5));
        // out-of-range selections are clamped, never panic
        let _ = toggle_alt("ab", 10, 20);
        let _ = toggle_bullets("ab", 10, 20);
    }
}
