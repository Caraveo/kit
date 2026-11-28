//! The x script editor window, with syntax highlighting.

use crate::state::Editor;
use crate::theme::*;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, CornerRadius, FontId, Frame, Id, Margin, RichText, Stroke};

const EDITOR_ID: &str = "x_code_editor";

/// Is the code editor's text field focused (so Ctrl+S saves the script)?
pub fn script_editor_focused(ctx: &egui::Context) -> bool {
    let id = Id::new(EDITOR_ID);
    ctx.memory(|m| m.focused().is_some_and(|f| f == id))
}

fn is_keyword(w: &str) -> bool {
    kit::x::lexer::Kw::from_str(w).is_some_and(|k| !k.is_type())
}

fn is_type(w: &str) -> bool {
    kit::x::lexer::Kw::from_str(w).is_some_and(|k| k.is_type())
}

fn is_module(w: &str) -> bool {
    kit::script::MODULES.contains(&w) || w == "Math"
}

/// Highlight x source into a layout job.
pub fn highlight(src: &str) -> LayoutJob {
    let font = FontId::monospace(13.0);
    let mut job = LayoutJob::default();
    let fmt = |c: Color32| TextFormat { font_id: font.clone(), color: c, ..Default::default() };
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut i = 0;
    let byte = |i: usize| chars.get(i).map(|c| c.0).unwrap_or(src.len());
    while i < chars.len() {
        let c = chars[i].1;
        let start = i;
        let color = if c == '#' {
            while i < chars.len() && chars[i].1 != '\n' {
                i += 1;
            }
            CODE_COMMENT()
        } else if c == '"' || c == '\'' {
            i += 1;
            while i < chars.len() && chars[i].1 != c && chars[i].1 != '\n' {
                if chars[i].1 == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(chars.len());
            CODE_STRING()
        } else if c.is_ascii_digit() {
            while i < chars.len() && (chars[i].1.is_ascii_alphanumeric() || chars[i].1 == '.' || chars[i].1 == '_') {
                i += 1;
            }
            CODE_NUMBER()
        } else if c.is_alphabetic() || c == '_' {
            while i < chars.len() && (chars[i].1.is_alphanumeric() || chars[i].1 == '_') {
                i += 1;
            }
            let w = &src[byte(start)..byte(i)];
            if is_type(w) {
                CODE_TYPE()
            } else if is_keyword(w) {
                CODE_KEYWORD()
            } else if is_module(w) {
                CODE_MODULE()
            } else {
                TEXT()
            }
        } else {
            i += 1;
            if "[](){}".contains(c) {
                LABEL()
            } else {
                if is_light() { MUTED() } else { Color32::from_rgb(0xb8, 0xb0, 0xcc) }
            }
        };
        job.append(&src[byte(start)..byte(i)], 0.0, fmt(color));
    }
    job
}

pub fn script_window(ctx: &egui::Context, ed: &mut Editor) {
    let Some(active) = ed.active_script else { return };
    if active >= ed.scripts.len() {
        ed.active_script = None;
        return;
    }
    let mut open = true;
    let mut save = None;
    let mut close = None;
    let mut switch = None;
    let title = format!("Script — {}", ed.scripts[active].path);
    let save_shortcut = ed.controls.label(crate::controls::Shortcut::Save);
    let close_button = ed.controls.gestures.close_script_tab.egui();
    egui::Window::new(RichText::new("Script").strong())
        .id(Id::new("script_window"))
        .open(&mut open)
        .title_bar(false)
        .default_size([600.0, 520.0])
        .default_pos([380.0, 120.0])
        .min_size([360.0, 240.0])
        .resizable(true)
        .collapsible(true)
        .frame(
            Frame::new()
                .fill(DIALOG_BG())
                .stroke(Stroke::new(1.0, DIALOG_BORDER()))
                .corner_radius(CornerRadius::same(15))
                .inner_margin(Margin::same(14)),
        )
        .show(ctx, |ui| {
            if crate::ui::popup_toolbar(ui, "Script") {
                close = Some(active);
            }
            let _ = &title;
            // Tabs.
            ui.horizontal_wrapped(|ui| {
                for (i, doc) in ed.scripts.iter().enumerate() {
                    let name = doc.path.rsplit('/').next().unwrap_or(&doc.path);
                    let label = if doc.dirty() { format!("{name} ●") } else { name.to_string() };
                    let selected = i == active;
                    let r = ui.add(egui::Button::selectable(selected, RichText::new(label).size(12.0)));
                    if r.clicked() {
                        switch = Some(i);
                    }
                    if r.clicked_by(close_button) {
                        close = Some(i);
                    }
                }
            });
            ui.add_space(4.0);
            let doc = &mut ed.scripts[active];
            ui.horizontal(|ui| {
                ui.label(RichText::new(&doc.path).size(11.0).color(MUTED()).monospace());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close = Some(active);
                    }
                    let save_label = if doc.dirty() { format!("Save  {save_shortcut}") } else { "Saved".into() };
                    if ui.add_enabled(doc.dirty(), egui::Button::new(save_label)).clicked() {
                        save = Some(active);
                    }
                });
            });
            if let Some(err) = &doc.error {
                Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(0xff, 0x8f, 0xa3, 0x18))
                    .corner_radius(CornerRadius::same(7))
                    .inner_margin(Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.label(RichText::new(err).size(12.0).color(ERROR()).monospace());
                    });
            }
            let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap: f32| {
                let mut job = highlight(text.as_str());
                job.wrap.max_width = wrap;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            egui::ScrollArea::both().id_salt(("code", active)).auto_shrink([false, false]).show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut doc.text)
                        .id(Id::new(EDITOR_ID))
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(24)
                        .lock_focus(true)
                        .layouter(&mut layouter),
                );
            });
        });
    if let Some(i) = switch {
        ed.active_script = Some(i);
    }
    if let Some(i) = save {
        ed.save_script(i);
    }
    if let Some(i) = close {
        if ed.scripts[i].dirty() {
            ed.save_script(i);
        }
        ed.scripts.remove(i);
        ed.active_script = if ed.scripts.is_empty() { None } else { Some(i.min(ed.scripts.len() - 1)) };
    }
    if !open {
        ed.active_script = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_covers_all_text() {
        let src = "TAKE Transform # hi\nFLOAT Speed: 2.5\nDISPLAY[\"a\\\"b\"]\n";
        let job = highlight(src);
        assert_eq!(job.text, src);
        let colored: Vec<(String, Color32)> = job
            .sections
            .iter()
            .map(|s| (job.text[s.byte_range.start.0..s.byte_range.end.0].to_string(), s.format.color))
            .collect();
        assert!(colored.contains(&("TAKE".into(), CODE_KEYWORD())));
        assert!(colored.contains(&("Transform".into(), CODE_MODULE())));
        assert!(colored.contains(&("FLOAT".into(), CODE_TYPE())));
        assert!(colored.contains(&("2.5".into(), CODE_NUMBER())));
        assert!(colored.contains(&("# hi".into(), CODE_COMMENT())));
    }
}
