//! Read-only decoding and playback support for Kit's asset preview.

use anyhow::{Context, Result, ensure};
use glam::Vec3;
use image::{DynamicImage, ImageReader};
use kit_resource::Resources;
use std::io::Cursor;
use std::path::Path;

pub mod video;

pub const IMAGES: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "bmp", "ico", "tif", "tiff", "tga", "dds", "hdr", "pnm", "pbm", "pgm", "ppm",
    "qoi", "svg",
];
pub const VIDEOS: &[&str] = &["mp4", "m4v", "webm", "mov", "avi", "mkv", "mpeg", "mpg", "ogv", "wmv"];
pub const TEXT: &[&str] = &["txt", "md", "json", "toml", "csv", "tsv", "xml", "yaml", "yml", "ini", "log"];
const MAX_READ: usize = 64 * 1024 * 1024;

pub fn extension(path: &str) -> String {
    Path::new(path).extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase()
}

pub fn image(bytes: &[u8], ext: &str) -> Result<DynamicImage> {
    if ext == "svg" {
        return svg(bytes);
    }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    if reader.format().is_none() {
        if let Some(format) = image::ImageFormat::from_extension(ext) {
            reader.set_format(format);
        }
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(192 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader.decode()?)
}

/// Read only enough of a binary or text asset to display it safely.
pub fn sample(resources: &Resources, asset: &str, limit: usize) -> Result<(Vec<u8>, bool)> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(resources.resolve(asset)?)?.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((bytes, truncated))
}

pub fn read_image(resources: &Resources, asset: &str) -> Result<DynamicImage> {
    let (bytes, truncated) = sample(resources, asset, MAX_READ)?;
    ensure!(!truncated, "This image exceeds the 64 MiB input limit");
    let ext = extension(asset);
    image(&bytes, &ext)
}

fn svg(bytes: &[u8]) -> Result<DynamicImage> {
    let mut options = resvg::usvg::Options::default();
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    let tree = resvg::usvg::Tree::from_data(bytes, &options).context("reading SVG")?;
    let size = tree.size();
    let scale = (2048.0 / size.width().max(size.height())).min(1.0);
    let width = (size.width() * scale).ceil().max(1.0) as u32;
    let height = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).context("allocating SVG preview")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect::<Vec<_>>();
    Ok(DynamicImage::ImageRgba8(image::RgbaImage::from_raw(width, height, pixels).context("reading SVG pixels")?))
}

#[derive(Clone, Copy)]
pub struct Triangle {
    pub positions: [Vec3; 3],
    pub color: [f32; 4],
}

pub struct Model {
    pub triangles: Vec<Triangle>,
    pub total: usize,
}

pub fn model(resources: &Resources, asset: &str) -> Result<Model> {
    let imported;
    let mut meshes = Vec::new();
    if kit_render::is_model_bundle(Path::new(asset)) {
        let model = kit_render::model_asset::ModelAsset::load(resources, asset)?;
        let frame = model
            .rest
            .and_then(|index| model.frames.get(index))
            .filter(|frame| frame.iter().any(Option::is_some))
            .or_else(|| model.frames.iter().find(|frame| frame.iter().any(Option::is_some)))
            .context("The model has no visible geometry")?;
        for (file, material) in frame.iter().zip(&model.materials) {
            if let Some(file) = file {
                let vertices = kit_render::model_asset::read_frame_vertices(resources, asset, file)?;
                let indices = (0..u32::try_from(vertices.len())?).collect();
                meshes.push((vertices.into_iter().map(|vertex| vertex.pos).collect(), indices, material.color));
            }
        }
    } else {
        imported = resources.load_model(asset)?;
        for part in imported.parts {
            meshes.push((part.positions, part.indices, part.color));
        }
    }

    let total = meshes.iter().map(|(_, indices, _)| indices.len() / 3).sum::<usize>();
    let step = total.div_ceil(24_000).max(1);
    let mut low = Vec3::splat(f32::INFINITY);
    let mut high = Vec3::splat(f32::NEG_INFINITY);
    for (positions, _, _) in &meshes {
        for &position in positions {
            let position = Vec3::from(position);
            if position.is_finite() {
                low = low.min(position);
                high = high.max(position);
            }
        }
    }
    let center = low * 0.5 + high * 0.5;
    let extent = (high * 0.5 - low * 0.5).max_element();
    ensure!(center.is_finite() && extent.is_finite() && extent > 0.0, "The model has invalid or empty bounds");
    let mut triangles = Vec::new();
    for (positions, indices, color) in meshes {
        for face in indices.chunks_exact(3).step_by(step) {
            let coords = |idx: u32| {
                positions
                    .get(idx as usize)
                    .copied()
                    .map(Vec3::from)
                    .filter(|point| point.is_finite())
                    .map(|point| (point - center) / extent)
            };
            if let (Some(a), Some(b), Some(c)) = (coords(face[0]), coords(face[1]), coords(face[2])) {
                triangles.push(Triangle { positions: [a, b, c], color });
            }
        }
    }
    ensure!(!triangles.is_empty(), "The model has no readable triangles");
    Ok(Model { triangles, total })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_hello_kit_model_bundles_and_glb_assets() {
        let resources = Resources::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello"));
        for file in [
            "models/Banana_20261123_030458_675940/Banana.kmodel",
            "models/Pineapple_20261123_030943_913589/Pineapple.kmodel",
            // The example no longer ships a .glb (dimension-logo.glb was removed).
            "models/Chip_20261124_164745_414191/Chip.kmodel",
            "models/Plasma_Entrance_20261123_233853_826053/Entrance.kmodel",
        ] {
            let model = model(&resources, file).unwrap_or_else(|error| panic!("{file}: {error:#}"));
            assert!(model.total > 0, "{file}");
            assert!(!model.triangles.is_empty(), "{file}");
        }
    }
}
