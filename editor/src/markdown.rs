//! A small Markdown renderer for chat text (Rex's replies): headings, bold,
//! italics, inline code, links, bullet and numbered lists, and quotes.
//! Fenced code blocks are handled by the caller.

use crate::theme::*;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, CornerRadius, FontId, Frame, Margin, RichText, Stroke, Ui};

/// Body text size.
const BODY: f32 = 13.5;

#[derive(Debug, PartialEq)]
enum Block<'a> {
    Heading(u8, &'a str),
    Bullet(usize, &'a str),
    Numbered(usize, String, &'a str),
    Quote(&'a str),
    Paragraph(String),
    Rule,
    Gap,
}

fn blocks(text: &str) -> Vec<Block<'_>> {
    let mut out: Vec<Block> = Vec::new();
    let mut para: Vec<&str> = Vec::new();
    let flush = |para: &mut Vec<&str>, out: &mut Vec<Block>| {
        if !para.is_empty() {
            out.push(Block::Paragraph(para.join(" ")));
            para.clear();
        }
    };
    for raw in text.lines() {
        let indent = raw.len() - raw.trim_start().len();
        let line = raw.trim();
        if line.is_empty() {
            flush(&mut para, &mut out);
            if !matches!(out.last(), Some(Block::Gap) | None) {
                out.push(Block::Gap);
            }
            continue;
        }
        let heading = line.chars().take_while(|c| *c == '#').count();
        if (1..=4).contains(&heading) && line[heading..].starts_with(' ') {
            flush(&mut para, &mut out);
            out.push(Block::Heading(heading as u8, line[heading..].trim()));
        } else if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")).or_else(|| line.strip_prefix("• ")) {
            flush(&mut para, &mut out);
            out.push(Block::Bullet(indent / 2, rest));
        } else if let Some((num, rest)) = numbered(line) {
            flush(&mut para, &mut out);
            out.push(Block::Numbered(indent / 2, num, rest));
        } else if let Some(rest) = line.strip_prefix('>') {
            flush(&mut para, &mut out);
            out.push(Block::Quote(rest.trim()));
        } else if line == "---" || line == "***" {
            flush(&mut para, &mut out);
            out.push(Block::Rule);
        } else {
            para.push(line);
        }
    }
    flush(&mut para, &mut out);
    while matches!(out.last(), Some(Block::Gap)) {
        out.pop();
    }
    out
}

fn numbered(line: &str) -> Option<(String, &str)> {
    let digits: String = line.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 3 {
        return None;
    }
    let rest = &line[digits.len()..];
    let rest = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") "))?;
    Some((digits, rest))
}

/// Inline formatting → a layout job, plus the links it contains.
fn inline(text: &str, size: f32, base: Color32, strong_base: bool) -> (LayoutJob, Vec<(String, String)>) {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let mut links = Vec::new();
    let fmt = |bold: bool, italic: bool, code: bool, link: bool| {
        let mut f = TextFormat {
            font_id: if code { FontId::monospace(size - 1.0) } else { FontId::proportional(size) },
            color: if link { ACCENT() } else if bold || strong_base { TEXT_STRONG() } else { base },
            italics: italic,
            ..Default::default()
        };
        if code {
            f.background = FIELD_BG();
            f.color = CODE_STRING();
        }
        if link {
            f.underline = Stroke::new(1.0, ACCENT().gamma_multiply(0.7));
        }
        if bold {
            // No bold face in the UI font: brighter and very slightly larger reads as bold.
            f.font_id = FontId::proportional(size + 0.4);
        }
        f
    };
    let chars: Vec<char> = text.chars().collect();
    let (mut bold, mut italic) = (false, false);
    let mut buf = String::new();
    let mut i = 0;
    let push = |job: &mut LayoutJob, buf: &mut String, f: TextFormat| {
        if !buf.is_empty() {
            job.append(buf, 0.0, f);
            buf.clear();
        }
    };
    while i < chars.len() {
        let c = chars[i];
        // `code`
        if c == '`' {
            if let Some(end) = chars[i + 1..].iter().position(|x| *x == '`') {
                push(&mut job, &mut buf, fmt(bold, italic, false, false));
                let code: String = chars[i + 1..i + 1 + end].iter().collect();
                job.append(&format!("\u{2009}{code}\u{2009}"), 0.0, fmt(false, false, true, false));
                i += end + 2;
                continue;
            }
        }
        // **bold** / __bold__
        if (c == '*' || c == '_') && chars.get(i + 1) == Some(&c) {
            push(&mut job, &mut buf, fmt(bold, italic, false, false));
            bold = !bold;
            i += 2;
            continue;
        }
        // *italic* / _italic_ (not inside words like snake_case)
        if (c == '*' || c == '_') && {
            let prev = if i > 0 { chars[i - 1] } else { ' ' };
            let next = chars.get(i + 1).copied().unwrap_or(' ');
            if italic { !next.is_alphanumeric() } else { !prev.is_alphanumeric() && !next.is_whitespace() }
        } {
            push(&mut job, &mut buf, fmt(bold, italic, false, false));
            italic = !italic;
            i += 1;
            continue;
        }
        // [text](url)
        if c == '[' {
            if let Some(close) = chars[i..].iter().position(|x| *x == ']') {
                let close = i + close;
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(end) = chars[close..].iter().position(|x| *x == ')') {
                        let end = close + end;
                        let label: String = chars[i + 1..close].iter().collect();
                        let url: String = chars[close + 2..end].iter().collect();
                        push(&mut job, &mut buf, fmt(bold, italic, false, false));
                        job.append(&label, 0.0, fmt(bold, italic, false, true));
                        links.push((label, url));
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        buf.push(c);
        i += 1;
    }
    push(&mut job, &mut buf, fmt(bold, italic, false, false));
    (job, links)
}

fn text_line(ui: &mut Ui, text: &str, size: f32, color: Color32, strong: bool) {
    let (mut job, links) = inline(text, size, color, strong);
    job.wrap.max_width = ui.available_width();
    ui.add(egui::Label::new(job).wrap());
    if !links.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for (label, url) in links {
                if url.starts_with("https://") || url.starts_with("http://") {
                    ui.hyperlink_to(RichText::new(format!("{label} ↗")).size(11.5).color(ACCENT()), url);
                }
            }
        });
    }
}

/// Replace or drop characters the editor's fonts can't draw (they'd show as
/// squares): newer emoji, emoji joiners and variation selectors, and a few
/// symbols with drawable look-alikes.
pub fn displayable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\u{FE0E}' | '\u{FE0F}' | '\u{200D}' | '\u{20E3}' => {}
            '\u{1F3FB}'..='\u{1F3FF}' => {} // skin tones
            '✓' | '☑' => out.push('✔'),
            '✗' | '✘' => out.push('×'),
            '🙂' | '🙃' | '😀' | '😃' | '😄' | '😁' => out.push('😊'),
            '🦴' => out.push('🐾'),
            '🕳' => {}
            '\u{1F900}'..='\u{1FAFF}' => {} // Unicode 9+ emoji: not in the bundled emoji font
            _ => out.push(c),
        }
    }
    out
}

/// Render Markdown text.
pub fn show(ui: &mut Ui, text: &str) {
    let text = &displayable(text);
    ui.spacing_mut().item_spacing.y = 4.0;
    for block in blocks(text) {
        match block {
            Block::Heading(level, t) => {
                ui.add_space(if level == 1 { 6.0 } else { 3.0 });
                let size = match level {
                    1 => 18.0,
                    2 => 16.0,
                    _ => 14.5,
                };
                text_line(ui, t, size, TEXT_STRONG(), true);
            }
            Block::Bullet(depth, t) => {
                ui.horizontal_top(|ui| {
                    ui.add_space(4.0 + depth as f32 * 16.0);
                    let size = if depth == 0 { 8.0 } else { 6.0 };
                    ui.label(RichText::new("●").size(size).color(ACCENT()));
                    ui.add_space(2.0);
                    ui.vertical(|ui| text_line(ui, t, BODY, TEXT(), false));
                });
            }
            Block::Numbered(depth, n, t) => {
                ui.horizontal_top(|ui| {
                    ui.add_space(2.0 + depth as f32 * 16.0);
                    Frame::new()
                        .fill(ACCENT().gamma_multiply(0.18))
                        .corner_radius(CornerRadius::same(9))
                        .inner_margin(Margin::symmetric(5, 0))
                        .show(ui, |ui| {
                            ui.label(RichText::new(n).size(11.0).strong().color(ACCENT()));
                        });
                    ui.vertical(|ui| text_line(ui, t, BODY, TEXT(), false));
                });
            }
            Block::Quote(t) => {
                Frame::new()
                    .fill(ACCENT().gamma_multiply(0.06))
                    .stroke(Stroke::NONE)
                    .corner_radius(CornerRadius { nw: 2, sw: 2, ne: 8, se: 8 })
                    .inner_margin(Margin { left: 12, right: 8, top: 4, bottom: 4 })
                    .show(ui, |ui| {
                        let r = ui.max_rect();
                        ui.painter().vline(r.left() - 10.0, r.y_range(), Stroke::new(3.0, ACCENT().gamma_multiply(0.6)));
                        text_line(ui, t, BODY, LABEL(), false);
                    });
            }
            Block::Paragraph(t) => text_line(ui, &t, BODY, TEXT(), false),
            Block::Rule => {
                ui.separator();
            }
            Block::Gap => ui.add_space(4.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_markdown_into_blocks() {
        let b = blocks("## Steps\n\n1. Add a **Collider**\n2. Tick `Trigger`\n\n- one\n  - nested\n> tip\nPlain text\ncontinues");
        assert_eq!(b[0], Block::Heading(2, "Steps"));
        assert_eq!(b[1], Block::Gap);
        assert_eq!(b[2], Block::Numbered(0, "1".into(), "Add a **Collider**"));
        assert_eq!(b[5], Block::Bullet(0, "one"));
        assert_eq!(b[6], Block::Bullet(1, "nested"));
        assert_eq!(b[7], Block::Quote("tip"));
        assert_eq!(b[8], Block::Paragraph("Plain text continues".into()));
    }

    #[test]
    fn undrawable_characters_are_swapped_or_dropped() {
        assert_eq!(displayable("Fetch 🦴 dig 🕳️ ok ✓ hi 🙂 bot 🤖 ❤️ 👍🏽"), "Fetch 🐾 dig  ok ✔ hi 😊 bot  ❤ 👍");
        assert_eq!(displayable("plain text, ünïcödé and → arrows"), "plain text, ünïcödé and → arrows");
    }

    #[test]
    fn inline_formatting_and_links() {
        let (job, links) = inline("Use **Speed** in `player_move.x`, see [docs](https://kit.example/x) or *maybe* not snake_case_name", 13.0, Color32::WHITE, false);
        let text: String = job.text.clone();
        assert!(text.contains("Speed") && !text.contains("**") && !text.contains('`'));
        assert!(text.contains("snake_case_name"), "underscores inside words stay");
        assert_eq!(links, vec![("docs".into(), "https://kit.example/x".into())]);
    }
}
