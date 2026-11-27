//! The in-game interface: each frame the active camera's interface is laid out
//! at the screen size, mouse hover/press/click is applied, buttons run their
//! script's `OnPress()`, and the result is tessellated into a [`UiFrame`] that
//! renderers (Vulkan, WebGPU, the editor's play view) draw on top of the scene.

use crate::doc::Document;
use crate::layout::{Item, Options, layout};
use crate::paint::{ElementState, UiFonts, paint};
use crate::{InterfaceRuntime, style};
use epaint::{Color32, Pos2, Rect, TextureId, Vec2, pos2, vec2};
use kit_ecs::{Entity, World};
use kit_gamesys::InterfaceAttachment;
use kit_resource::Resources;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

/// The surface interfaces are drawn on, set by the host each frame.
#[derive(Clone, Copy, Debug)]
pub struct UiScreen {
    /// Size in points (logical pixels).
    pub size: [f32; 2],
    /// Physical pixels per point.
    pub pixels_per_point: f32,
    /// `Input::mouse` → points: `(mouse - origin) * scale`.
    pub mouse_origin: [f32; 2],
    pub mouse_scale: f32,
}

impl UiScreen {
    /// A window: mouse in physical pixels from the top-left.
    pub fn window(physical: [u32; 2], pixels_per_point: f32) -> Self {
        let ppp = pixels_per_point.max(0.1);
        Self {
            size: [physical[0] as f32 / ppp, physical[1] as f32 / ppp],
            pixels_per_point: ppp,
            mouse_origin: [0.0, 0.0],
            mouse_scale: 1.0 / ppp,
        }
    }
}

/// One UI vertex: position in points, texture coordinates, premultiplied sRGBA.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct UiVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [u8; 4],
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum UiTexture {
    /// The font atlas ([`UiFrame::atlas`]).
    Font,
    /// A project image by path.
    Image(String),
}

#[derive(Clone, Debug)]
pub struct UiMesh {
    /// Clip rectangle in points: min x, min y, max x, max y.
    pub clip: [f32; 4],
    pub texture: UiTexture,
    pub vertices: Vec<UiVertex>,
    pub indices: Vec<u32>,
}

/// The font atlas as premultiplied sRGBA pixels.
#[derive(Clone, Debug, Default)]
pub struct UiAtlas {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
    /// Changes whenever the pixels change (renderers re-upload).
    pub revision: u64,
}

/// What to draw this frame.
#[derive(Clone, Debug, Default)]
pub struct UiFrame {
    pub meshes: Vec<UiMesh>,
    pub atlas: Arc<UiAtlas>,
    pub size: [f32; 2],
    pub pixels_per_point: f32,
    /// The pointer is over an interface element (games can ignore clicks there).
    pub captures_mouse: bool,
}

/// Cached documents and interaction state.
#[derive(Default)]
pub struct UiHost {
    fonts: UiFonts,
    docs: HashMap<String, (Option<SystemTime>, Option<Arc<Document>>)>,
    image_sizes: HashMap<String, Vec2>,
    hovered: Vec<Vec<usize>>,
    pressed: Option<Vec<usize>>,
    camera: Option<Entity>,
    atlas: UiAtlas,
    /// Last error per interface (shown once in the log).
    errors: HashMap<String, String>,
}

impl UiHost {
    fn doc(&mut self, res: &Resources, path: &str) -> Option<Arc<Document>> {
        let modified = res.modified(path);
        if let Some((m, doc)) = self.docs.get(path) {
            if *m == modified {
                return doc.clone();
            }
        }
        let doc = match res.read_text(path).and_then(|t| Document::parse(&t)) {
            Ok(d) => Some(Arc::new(d)),
            Err(e) => {
                let msg = format!("{path}: {e:#}");
                if self.errors.get(path) != Some(&msg) {
                    log::warn!("interface {msg}");
                    self.errors.insert(path.to_owned(), msg);
                }
                None
            }
        };
        self.docs.insert(path.to_owned(), (modified, doc.clone()));
        doc
    }

    fn image_size(&mut self, res: &Resources, path: &str) -> Vec2 {
        if let Some(s) = self.image_sizes.get(path) {
            return *s;
        }
        let size = res
            .read_bytes(path)
            .ok()
            .and_then(|b| {
                image::ImageReader::new(std::io::Cursor::new(b)).with_guessed_format().ok()?.into_dimensions().ok()
            })
            .map_or(Vec2::ZERO, |(w, h)| vec2(w as f32, h as f32));
        self.image_sizes.insert(path.to_owned(), size);
        size
    }

    fn apply_atlas(&mut self) {
        let Some(delta) = self.fonts.atlas_delta() else { return };
        let epaint::ImageData::Color(image) = &delta.image;
        let [w, h] = image.size;
        let [x0, y0] = delta.pos.unwrap_or([0, 0]);
        if delta.pos.is_none() {
            self.atlas.size = [w, h];
            self.atlas.rgba = vec![0; w * h * 4];
        }
        let stride = self.atlas.size[0];
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x0 + x, y0 + y);
                if dx >= self.atlas.size[0] || dy >= self.atlas.size[1] {
                    continue;
                }
                let c: Color32 = image.pixels[y * w + x];
                let i = (dy * stride + dx) * 4;
                self.atlas.rgba[i..i + 4].copy_from_slice(&c.to_array());
            }
        }
        self.atlas.revision += 1;
    }
}

/// The script a button runs: `Resume` / `Resume.x` → `scripts/Resume.x`.
pub fn script_path(script: &str) -> String {
    let mut path = script.trim().to_owned();
    if !path.ends_with(".x") {
        path.push_str(".x");
    }
    if !path.contains('/') {
        path = format!("scripts/{path}");
    }
    path
}

/// Letterbox bars, a fade to black and a caption, as interface elements on top.
fn overlay_nodes(o: &kit_gamesys::CinematicOverlay) -> Vec<crate::Node> {
    let esc = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let bar = (o.letterbox.clamp(0.0, 1.0) * 11.0).max(0.0);
    let fade = (o.fade.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut src = String::from("<interface>");
    if bar > 0.01 {
        src += &format!(
            r##"<panel id="__cinematic-top" style="position: absolute; x: 0; y: 0; width: 100%; height: {bar}%; background: #000000ff; border-radius: 0;" />"##
        );
        src += &format!(
            r##"<panel id="__cinematic-bottom" style="position: absolute; x: 0; y: {}%; width: 100%; height: {bar}%; background: #000000ff; border-radius: 0;" />"##,
            100.0 - bar
        );
    }
    if !o.caption.is_empty() {
        src += &format!(
            r##"<panel id="__cinematic-caption" style="position: absolute; x: 0; y: 90%; width: 100%; height: 7%; background: #00000000; border-radius: 0;" align="center"><label text="{}" font-size="1.35em" color="#f4f6ffff" /></panel>"##,
            esc(&o.caption)
        );
    }
    if fade > 0 {
        src += &format!(
            r##"<panel id="__cinematic-fade" style="position: absolute; x: 0; y: 0; width: 100%; height: 100%; background: #000000{fade:02x}; border-radius: 0;" />"##
        );
    }
    src += "</interface>";
    Document::parse(&src).map(|d| d.root.children).unwrap_or_default()
}

/// Set attributes scripts changed on elements with matching ids.
fn apply_edits(node: &mut crate::Node, edits: &HashMap<String, Vec<(String, String)>>) {
    if let Some(list) = node.id().and_then(|id| edits.get(id)) {
        for (attr, value) in list.clone() {
            node.set_attr(attr, value);
        }
    }
    for child in &mut node.children {
        apply_edits(child, edits);
    }
}

fn is_button(item: &Item) -> bool {
    item.node.tag == "button"
}

/// The play-mode system.
pub fn update_ui(world: &mut World) {
    let Some(screen) = world.try_resource::<UiScreen>().map(|s| *s) else { return };
    let Some(res) = world.try_resource::<Resources>().map(|r| r.clone()) else { return };
    if world.try_resource::<UiHost>().is_none() {
        world.insert_resource(UiHost::default());
    }
    let camera = kit_gamesys::active_camera(world);
    let asset =
        camera.and_then(|c| world.get::<InterfaceAttachment>(c).map(|a| a.asset.clone())).filter(|a| !a.is_empty());
    // A cinematic's letterbox, fade and caption draw even without an interface.
    let overlay = world.try_resource::<kit_gamesys::CinematicOverlay>().filter(|o| o.is_visible()).map(|o| o.clone());
    if asset.is_none() && overlay.is_none() {
        world.insert_resource(UiFrame {
            size: screen.size,
            pixels_per_point: screen.pixels_per_point,
            ..Default::default()
        });
        return;
    }
    let mut visible = camera
        .and_then(|camera| {
            world
                .try_resource::<InterfaceRuntime>()
                .and_then(|r| r.instances.get(&camera).map(|i| (i.visible.clone(), i.classes.clone())))
        })
        .unwrap_or_default();
    // Scripts' Interface.Show / Hide / SetText / SetColor.
    let edits = world.try_resource::<kit_gamesys::InterfaceEdits>().map(|e| e.clone()).unwrap_or_default();
    visible.0.extend(edits.visible.iter().map(|(k, v)| (k.clone(), *v)));
    let (mouse, down, pressed_now, released_now, keys) = match world.try_resource::<kit_input::Input>() {
        Some(i) => {
            let m = pos2(
                (i.mouse.x - screen.mouse_origin[0]) * screen.mouse_scale,
                (i.mouse.y - screen.mouse_origin[1]) * screen.mouse_scale,
            );
            (Some(m), i.down("MouseLeft"), i.pressed("MouseLeft"), i.released("MouseLeft"), i.pressed_keys())
        }
        None => (None, false, false, false, Vec::new()),
    };

    let mut clicks: Vec<(Option<String>, Vec<usize>, Option<String>)> = Vec::new();
    let frame = {
        let mut host = world.resource_mut::<UiHost>();
        if host.camera != camera {
            host.camera = camera;
            host.hovered.clear();
            host.pressed = None;
        }
        let doc = match &asset {
            Some(asset) => host.doc(&res, asset),
            None => Some(Arc::new(Document::parse("<interface />").expect("empty interface"))),
        };
        let Some(mut doc) = doc
            .or_else(|| overlay.as_ref().map(|_| Arc::new(Document::parse("<interface />").expect("empty interface"))))
        else {
            drop(host);
            world.insert_resource(UiFrame {
                size: screen.size,
                pixels_per_point: screen.pixels_per_point,
                ..Default::default()
            });
            return;
        };
        if let Some(o) = &overlay {
            let mut with = (*doc).clone();
            with.root.children.extend(overlay_nodes(o));
            doc = Arc::new(with);
        }
        if !edits.attributes.is_empty() {
            let mut edited = (*doc).clone();
            apply_edits(&mut edited.root, &edits.attributes);
            doc = Arc::new(edited);
        }
        let dir = asset.as_deref().and_then(|a| a.rsplit_once('/')).map(|(d, _)| d.to_owned());
        let res2 = res.clone();
        let include = move |p: &str| {
            let full =
                if p.contains('/') { p.to_owned() } else { dir.as_ref().map_or(p.to_owned(), |d| format!("{d}/{p}")) };
            res2.read_text(&full).ok().and_then(|t| Document::parse(&t).ok())
        };
        let screen_rect = Rect::from_min_size(Pos2::ZERO, vec2(screen.size[0], screen.size[1]));
        let host = &mut *host;
        host.fonts.begin_frame();
        let l = {
            let mut view = host.fonts.view(screen.pixels_per_point);
            layout(&doc, screen_rect, &mut view, &Options { visible: Some(&visible.0), include: &include })
        };

        // Interaction.
        let over: Vec<&Item> = mouse
            .map(|m| l.items.iter().filter(|i| i.rect.contains(m) && i.clip.contains(m)).collect())
            .unwrap_or_default();
        let top_button = over.iter().rev().find(|i| is_button(i)).map(|i| i.path.clone());
        host.hovered = over.iter().map(|i| i.path.clone()).collect();
        if pressed_now {
            host.pressed = top_button.clone();
        }
        if released_now {
            if let (Some(p), Some(t)) = (host.pressed.take(), top_button.as_ref()) {
                if &p == t {
                    if let Some(item) = l.items.iter().find(|i| &i.path == t && is_button(i)) {
                        clicks.push((
                            item.node.attr("script").map(str::to_owned),
                            item.path.clone(),
                            item.node.id().map(str::to_owned),
                        ));
                    }
                }
            }
        } else if !down {
            host.pressed = None;
        }
        // Keyboard shortcuts: <button key="Escape">.
        for item in l.items.iter().filter(|i| is_button(i)) {
            if let Some(k) = item.node.attr("key") {
                if keys.iter().any(|pressed| kit_input::normalize_key(pressed) == kit_input::normalize_key(k)) {
                    clicks.push((
                        item.node.attr("script").map(str::to_owned),
                        item.path.clone(),
                        item.node.id().map(str::to_owned),
                    ));
                }
            }
        }

        let hovered = host.hovered.clone();
        let pressed = host.pressed.clone();
        let classes = visible.1.clone();
        let state = move |item: &Item| ElementState {
            hovered: hovered.contains(&item.path),
            pressed: pressed.as_ref() == Some(&item.path),
            focused: false,
            classes: item.node.id().and_then(|id| classes.get(id).cloned()).unwrap_or_default(),
        };
        let mut images: Vec<String> = Vec::new();
        let mut sizes: Vec<(String, Vec2)> = Vec::new();
        for item in l.items.iter().filter(|i| i.node.tag == "image") {
            if let Some(src) = item.node.attr("src").or_else(|| item.node.attr("sprite")).filter(|s| !s.is_empty()) {
                if !sizes.iter().any(|(s, _)| s == src) {
                    let size = host.image_size(&res, src);
                    sizes.push((src.to_owned(), size));
                }
            }
        }
        let shapes = {
            let mut view = host.fonts.view(screen.pixels_per_point);
            paint(&l, &mut view, &state, &mut |src: &str| {
                let index = images.iter().position(|s| s == src).unwrap_or_else(|| {
                    images.push(src.to_owned());
                    images.len() - 1
                });
                let size = sizes.iter().find(|(s, _)| s == src).map_or(Vec2::ZERO, |(_, v)| *v);
                Some((TextureId::User(index as u64), size))
            })
        };
        let prims = host.fonts.tessellate(shapes, screen.pixels_per_point);
        host.apply_atlas();
        let meshes = prims
            .into_iter()
            .filter_map(|p| {
                let epaint::Primitive::Mesh(mesh) = p.primitive else { return None };
                if mesh.indices.is_empty() {
                    return None;
                }
                let texture = match mesh.texture_id {
                    TextureId::User(i) => UiTexture::Image(images.get(i as usize)?.clone()),
                    TextureId::Managed(_) => UiTexture::Font,
                };
                Some(UiMesh {
                    clip: [p.clip_rect.min.x, p.clip_rect.min.y, p.clip_rect.max.x, p.clip_rect.max.y],
                    texture,
                    vertices: mesh
                        .vertices
                        .iter()
                        .map(|v| UiVertex { pos: [v.pos.x, v.pos.y], uv: [v.uv.x, v.uv.y], color: v.color.to_array() })
                        .collect(),
                    indices: mesh.indices,
                })
            })
            .collect();
        let captures = over
            .iter()
            .any(|i| style::prop(&i.node, "background").is_some() || matches!(i.node.tag.as_str(), "button" | "panel"));
        UiFrame {
            meshes,
            atlas: Arc::new(host.atlas.clone()),
            size: screen.size,
            pixels_per_point: screen.pixels_per_point,
            captures_mouse: captures,
        }
    };
    world.insert_resource(frame);

    // Run button actions after releasing borrows.
    let (Some(camera), Some(asset)) = (camera, asset) else { return };
    for (script, path, id) in clicks {
        if let Some(id) = &id {
            let doc = world.resource_mut::<UiHost>().doc(&res, &asset);
            if let Some(node) = doc.as_ref().and_then(|d| d.root.at(&path).cloned()) {
                for logic in node
                    .children
                    .iter()
                    .filter(|c| c.tag == "logic" && c.attr("on").is_some_and(|o| o.eq_ignore_ascii_case("press")))
                {
                    for class in logic.children.iter().filter(|a| a.tag == "add-class").filter_map(|a| a.attr("name")) {
                        crate::activate_class(world, camera, id, class);
                    }
                }
            }
        }
        if let Some(script) = script.filter(|s| !s.trim().is_empty()) {
            kit_script::call_file_hook(world, camera, &script_path(&script), "OnPress");
        }
    }
}
