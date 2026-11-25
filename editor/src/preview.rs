//! The center-pane asset viewer. Opening a preview never edits the scene.

use crate::{state::Editor, theme::*};
use anyhow::{Context, Result, ensure};
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, Ui, Vec2};
use kit::{math::{Quat, Vec3}, preview, resource::Resources};
use std::{path::PathBuf, sync::mpsc, time::Duration};

enum Content {
    Loading(mpsc::Receiver<Result<Content>>),
    Image { pixels: Option<image::RgbaImage>, texture: Option<TextureHandle> },
    Model(preview::Model),
    Audio { peaks: Vec<f32>, duration: f32 },
    Video { file: PathBuf, player: Option<preview::video::Video>, texture: Option<TextureHandle>, note: String },
    Text { text: String, format: TextFormat, truncated: bool },
    Binary { bytes: Vec<u8>, truncated: bool },
    Error(String),
}

enum TextFormat {
    Plain,
    Markdown,
    Script,
}

pub struct AssetPreview {
    pub path: String,
    content: Content,
    orbit: Vec2,
    zoom: f32,
}

impl AssetPreview {
    pub fn open(resources: Resources, path: &str) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        let asset = path.to_owned();
        let content = match std::thread::Builder::new().name("kit-asset-preview".into()).spawn(move || {
            let _ = sender.send(load(&resources, &asset));
        }) {
            Ok(_) => Content::Loading(receiver),
            Err(error) => Content::Error(error.to_string()),
        };
        Self { path: path.to_owned(), content, orbit: Vec2::new(-0.6, 0.3), zoom: 1.0 }
    }

    pub fn show(&mut self, ui: &mut Ui, ed: &mut Editor) {
        if let Content::Loading(receiver) = &self.content {
            match receiver.try_recv() {
                Ok(result) => self.content = result.unwrap_or_else(|error| Content::Error(format!("{error:#}"))),
                Err(mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(Duration::from_millis(30)),
                Err(mpsc::TryRecvError::Disconnected) => self.content = Content::Error("The preview could not be loaded.".into()),
            }
        }
        ui.push_id(&self.path, |ui| {
            ui.label(RichText::new(&self.path).color(MUTED()).size(12.0));
            ui.add_space(8.0);
            match &mut self.content {
                Content::Loading(_) => { ui.spinner(); }
                Content::Error(error) => { ui.colored_label(ERROR(), error); }
                Content::Image { pixels, texture } => {
                    if let Some(pixels) = pixels.take() { upload(ui, texture, pixels); }
                    if let Some(texture) = texture {
                        let size = texture.size();
                        ui.label(format!("{} × {}", size[0], size[1]));
                        show_image(ui, texture);
                    }
                }
                Content::Model(model) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("{} triangles", model.total));
                        if model.triangles.len() < model.total { ui.label("Sampled geometry"); }
                        ui.label("Drag to orbit · Scroll to zoom");
                        if ui.small_button("Reset view").clicked() {
                            self.orbit = Vec2::new(-0.6, 0.3);
                            self.zoom = 1.0;
                        }
                    });
                    show_model(ui, model, &mut self.orbit, &mut self.zoom);
                }
                Content::Audio { peaks, duration } => {
                    let playing = ed.preview_playing() == Some(self.path.as_str());
                    ui.horizontal(|ui| {
                        if ui.button(if playing { "Stop" } else { "Play" }).clicked() {
                            ed.toggle_preview(&self.path);
                        }
                        ui.label(format!("{duration:.1} s"));
                    });
                    if playing { ui.ctx().request_repaint_after(Duration::from_millis(100)); }
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 136.0), Sense::hover());
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 8.0, STAGE_BG());
                    for (i, peak) in peaks.iter().enumerate() {
                        let x = rect.left() + (i as f32 + 0.5) / peaks.len() as f32 * rect.width();
                        let h = peak * rect.height() * 0.43;
                        painter.line_segment([Pos2::new(x, rect.center().y - h), Pos2::new(x, rect.center().y + h)], Stroke::new(1.0, ACCENT()));
                    }
                }
                Content::Video { file, player, texture, note } => {
                    ui.horizontal(|ui| {
                        if ui.button(if player.is_some() { "Stop" } else { "Play" }).clicked() {
                            if player.take().is_some() {
                                ed.stop_preview();
                                note.clear();
                            } else {
                                ed.stop_preview();
                                let audio_rate = ed.app.world.try_resource_mut::<kit::audio::Audio>()
                                    .and_then(|mut audio| audio.preview_output_rate());
                                match preview::video::Video::start(file, audio_rate) {
                                    Ok(mut video) => {
                                        if let Some(receiver) = video.take_audio() {
                                            if let Some(mut audio) = ed.app.world.try_resource_mut::<kit::audio::Audio>() {
                                                audio.preview_stream(receiver);
                                            }
                                        }
                                        note.clear();
                                        *player = Some(video);
                                    }
                                    Err(error) => *note = format!("{error:#}"),
                                }
                            }
                        }
                        ui.label(note.as_str());
                    });
                    if let Some(video) = player {
                        if let Some(frame) = video.frame() { upload(ui, texture, frame); }
                        match video.finished() {
                            Ok(None) => ui.ctx().request_repaint_after(Duration::from_millis(25)),
                            Ok(Some(ok)) => {
                                *player = None;
                                *note = if ok { "Playback ended".into() } else { "The local decoder could not play this video.".into() };
                            }
                            Err(error) => { *player = None; *note = error.to_string(); }
                        }
                    }
                    if let Some(texture) = texture { show_image(ui, texture); }
                }
                Content::Text { text, format, truncated } => {
                    if *truncated { ui.label("Showing the first 256 KiB"); }
                    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                        match format {
                            TextFormat::Markdown => {
                                ui.set_max_width(ui.available_width().min(840.0));
                                show_markdown(ui, text);
                            }
                            TextFormat::Script => {
                                ui.add(egui::Label::new(crate::code::highlight(text)).selectable(true).wrap_mode(egui::TextWrapMode::Extend));
                            }
                            TextFormat::Plain => {
                                let mut text = text.as_str();
                                ui.add(egui::TextEdit::multiline(&mut text).font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY));
                            }
                        }
                    });
                }
                Content::Binary { bytes, truncated } => {
                    ui.label(if *truncated { "Binary file · first 256 bytes" } else { "Binary file" });
                    for (offset, row) in bytes.chunks(16).enumerate() {
                        let hex = row.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(" ");
                        ui.monospace(format!("{:04x}   {hex}", offset * 16));
                    }
                }
            }
        });
    }
}

fn load(resources: &Resources, asset: &str) -> Result<Content> {
    let ext = preview::extension(asset);
    if preview::IMAGES.contains(&ext.as_str()) {
        let image = preview::read_image(resources, asset)?;
        let image = if image.width().max(image.height()) > 2048 { image.resize(2048, 2048, image::imageops::FilterType::Triangle) } else { image };
        return Ok(Content::Image { pixels: Some(image.into_rgba8()), texture: None });
    }
    if matches!(ext.as_str(), "ttf" | "otf") {
        let (bytes, truncated) = preview::sample(resources, asset, 32 * 1024 * 1024)?;
        ensure!(!truncated, "This font exceeds the 32 MiB preview limit");
        return Ok(Content::Image { pixels: Some(font_sample(bytes)?), texture: None });
    }
    if crate::model_import::EXTENSIONS.contains(&ext.as_str()) {
        let model = if matches!(ext.as_str(), "fbx" | "usdz" | "stl") {
            let temp = tempfile::tempdir()?;
            let manifest = crate::model_import::convert(&resources.resolve(asset)?, temp.path())?;
            preview::model(&Resources::new(temp.path()), manifest.file_name().and_then(|name| name.to_str()).context("Missing converted model name")?)?
        } else {
            preview::model(resources, asset)?
        };
        return Ok(Content::Model(model));
    }
    if preview::VIDEOS.contains(&ext.as_str()) {
        return Ok(Content::Video { file: resources.resolve(asset)?.canonicalize()?, player: None, texture: None, note: String::new() });
    }
    if kit::gamesys::is_sound(asset) {
        let (bytes, truncated) = preview::sample(resources, asset, 64 * 1024 * 1024)?;
        ensure!(!truncated, "This audio file exceeds the 64 MiB preview limit");
        let clip = kit::audio::decode(bytes, &ext)?;
        let peaks = clip.frames.chunks(clip.frames.len().div_ceil(256).max(1)).map(|chunk| {
            chunk.iter().flatten().map(|sample| sample.abs()).fold(0.0_f32, f32::max).min(1.0)
        }).collect();
        return Ok(Content::Audio { peaks, duration: clip.duration() });
    }
    let (mut bytes, truncated) = preview::sample(resources, asset, 256 * 1024)?;
    if std::str::from_utf8(&bytes).is_err_and(|error| error.error_len().is_none()) && truncated {
        bytes.truncate(std::str::from_utf8(&bytes).unwrap_err().valid_up_to());
    }
    if let Ok(text) = std::str::from_utf8(&bytes) {
        if !text.contains('\0') {
            let format = match ext.as_str() {
                "md" => TextFormat::Markdown,
                "x" => TextFormat::Script,
                _ => TextFormat::Plain,
            };
            return Ok(Content::Text { text: text.to_owned(), format, truncated });
        }
    }
    let truncated = truncated || bytes.len() > 256;
    bytes.truncate(256);
    Ok(Content::Binary { bytes, truncated })
}

fn font_sample(bytes: Vec<u8>) -> Result<image::RgbaImage> {
    use ab_glyph::{Font, FontArc, ScaleFont, point};
    let font = FontArc::try_from_vec(bytes).context("reading font preview")?;
    let mut image = image::RgbaImage::from_pixel(960, 288, image::Rgba([26, 25, 35, 255]));
    for (text, size, baseline) in [
        ("The quick brown fox", 62.0, 82.0),
        ("ABCDEFGHIJKLMNOPQRSTUVWXYZ", 38.0, 160.0),
        ("abcdefghijklmnopqrstuvwxyz 0123456789", 35.0, 232.0),
    ] {
        let scaled = font.as_scaled(size);
        let mut x = 35.0;
        let mut previous = None;
        for ch in text.chars() {
            let id = scaled.glyph_id(ch);
            if let Some(previous) = previous { x += scaled.kern(previous, id); }
            let glyph = id.with_scale_and_position(size, point(x, baseline));
            if let Some(outline) = font.outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                outline.draw(|x, y, alpha| {
                    let x = (bounds.min.x + x as f32) as u32;
                    let y = (bounds.min.y + y as f32) as u32;
                    if let Some(pixel) = image.get_pixel_mut_checked(x, y) {
                        for (channel, color) in pixel.0[..3].iter_mut().zip([244.0, 238.0, 248.0]) {
                            *channel = (*channel as f32 * (1.0 - alpha) + color * alpha) as u8;
                        }
                    }
                });
            }
            x += scaled.h_advance(id);
            previous = Some(id);
        }
    }
    Ok(image)
}

fn upload(ui: &Ui, texture: &mut Option<TextureHandle>, pixels: image::RgbaImage) {
    let image = egui::ColorImage::from_rgba_unmultiplied([pixels.width() as usize, pixels.height() as usize], &pixels);
    if let Some(texture) = texture {
        texture.set(image, egui::TextureOptions::LINEAR);
    } else {
        *texture = Some(ui.ctx().load_texture("asset-preview", image, egui::TextureOptions::LINEAR));
    }
}

fn show_image(ui: &mut Ui, texture: &TextureHandle) {
    let available = ui.available_size();
    let size = texture.size_vec2();
    let scale = (available.x / size.x).min(available.y / size.y).min(2.0);
    if scale <= 0.0 { return; }
    let (rect, _) = ui.allocate_exact_size(available, Sense::hover());
    let painter = ui.painter_at(rect);
    let image_rect = Rect::from_center_size(rect.center(), size * scale);
    painter.rect_filled(image_rect, 0.0, Color32::from_gray(45));
    for (row, y) in (0..(image_rect.height() / 12.0).ceil() as usize).enumerate() {
        for x in (row % 2..(image_rect.width() / 12.0).ceil() as usize).step_by(2) {
            let tile = Rect::from_min_size(image_rect.min + Vec2::new(x as f32, y as f32) * 12.0, Vec2::splat(12.0));
            painter.rect_filled(tile.intersect(image_rect), 0.0, Color32::from_gray(53));
        }
    }
    painter.image(texture.id(), image_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
}

fn show_model(ui: &mut Ui, model: &preview::Model, orbit: &mut Vec2, zoom: &mut f32) {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::drag());
    if !rect.is_positive() { return; }
    *orbit += response.drag_delta() * 0.012;
    orbit.y = orbit.y.clamp(-1.56, 1.56);
    if response.hovered() {
        let scroll = ui.input(|input| input.smooth_scroll_delta.y);
        *zoom = (*zoom * (scroll * 0.004).exp()).clamp(0.3, 5.0);
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, STAGE_BG());
    let rotation = Quat::from_rotation_x(orbit.y) * Quat::from_rotation_y(orbit.x);
    let scale = rect.width().min(rect.height()) * 0.30 * *zoom;
    let light = Vec3::new(-0.4, 0.7, 0.6).normalize();
    let mut faces = Vec::with_capacity(model.triangles.len());
    for triangle in &model.triangles {
        let points = triangle.positions.map(|point| rotation * point);
        let normal = (points[1] - points[0]).cross(points[2] - points[0]);
        if normal.z <= 0.0 { continue; }
        let shade = normal.normalize_or_zero().dot(light).max(0.0) * 0.58 + 0.42;
        let [r, g, b, _] = triangle.color;
        let color = Color32::from_rgb(
            (r.clamp(0.0, 1.0) * shade * 255.0) as u8,
            (g.clamp(0.0, 1.0) * shade * 255.0) as u8,
            (b.clamp(0.0, 1.0) * shade * 255.0) as u8,
        );
        let screen = points.map(|point| rect.center() + Vec2::new(point.x, -point.y) * scale);
        faces.push((points.iter().map(|point| point.z).sum::<f32>(), screen, color));
    }
    faces.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let mut mesh = egui::Mesh::default();
    for (_, screen, color) in faces {
        let index = mesh.vertices.len() as u32;
        for point in screen { mesh.colored_vertex(point, color); }
        mesh.add_triangle(index, index + 1, index + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
}

fn show_markdown(ui: &mut Ui, text: &str) {
    let mut lines = text.lines().peekable();
    let mut prose = String::new();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if let Some(fence) = trimmed.strip_prefix("```").or_else(|| trimmed.strip_prefix("~~~")) {
            crate::markdown::show(ui, &std::mem::take(&mut prose));
            let delimiter = &trimmed[..3];
            let mut code = String::new();
            for line in lines.by_ref() {
                if line.trim_start().starts_with(delimiter) { break; }
                code.push_str(line);
                code.push('\n');
            }
            crate::theme::card().show(ui, |ui| {
                if !fence.trim().is_empty() { ui.label(RichText::new(fence.trim()).color(MUTED())); }
                let mut text = code.as_str();
                ui.add(egui::TextEdit::multiline(&mut text).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
            });
        } else {
            prose.push_str(line);
            prose.push('\n');
        }
    }
    crate::markdown::show(ui, &prose);
}
