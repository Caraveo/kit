//! Portable imported models with material slots and sampled animation clips.

use anyhow::{Context, Result, ensure};
use glam::Vec3;
use kit_resource::Resources;
use serde::Deserialize;
use std::path::{Component, Path};

#[derive(Deserialize)]
pub struct Material {
    pub color: [f32; 4],
    pub texture: Option<String>,
    #[serde(default)]
    pub blended: bool,
    #[serde(default)]
    pub unlit: bool,
    #[serde(default)]
    pub cutoff: f32,
    pub roughness: f32,
    pub metallic: f32,
    pub emission: [f32; 3],
    #[serde(default)]
    pub double_sided: bool,
}

#[derive(Deserialize)]
pub struct Clip {
    pub name: String,
    pub fps: f64,
    pub first: usize,
    pub count: usize,
}

#[derive(Deserialize)]
pub struct ModelAsset {
    pub materials: Vec<Material>,
    pub frames: Vec<Vec<Option<String>>>,
    pub clips: Vec<Clip>,
    pub bounds: [[f32; 3]; 2],
    #[serde(default)]
    pub warnings: Vec<String>,
    /// The frame holding the undeformed rest (bind) pose, when the importer
    /// saved one (rigs deform this instead of an animation frame).
    #[serde(default)]
    pub rest: Option<usize>,
}

impl ModelAsset {
    pub fn load(resources: &Resources, path: &str) -> Result<Self> {
        let data: Self =
            serde_json::from_str(&resources.read_text(path)?).context("reading imported model manifest")?;
        ensure!(
            data.frames.iter().flatten().any(Option::is_some) && !data.materials.is_empty(),
            "empty imported model"
        );
        let (lo, hi) = data.bounds();
        ensure!(lo.is_finite() && hi.is_finite() && lo.cmple(hi).all(), "invalid imported model bounds");
        ensure!(data.frames.iter().all(|f| f.len() == data.materials.len()), "invalid material slots");
        ensure!(
            data.clips.iter().all(|c| c.count > 0
                && c.fps.is_finite()
                && c.fps > 0.0
                && c.first.checked_add(c.count).is_some_and(|end| end <= data.frames.len())),
            "invalid animation clip"
        );
        for name in
            data.frames.iter().flatten().flatten().chain(data.materials.iter().filter_map(|m| m.texture.as_ref()))
        {
            let mut components = Path::new(name).components();
            ensure!(
                matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none(),
                "invalid imported model file name"
            );
        }
        Ok(data)
    }

    pub fn bounds(&self) -> (Vec3, Vec3) {
        (self.bounds[0].into(), self.bounds[1].into())
    }

    pub fn frame(&self, clip: &str, elapsed: f64, speed: f32, looping: bool) -> usize {
        let clip = if clip.is_empty() { self.clips.first() } else { self.clips.iter().find(|c| c.name == clip) };
        let Some(clip) = clip else { return 0 };
        let sample = (elapsed * f64::from(speed.max(0.0)) * clip.fps) as usize;
        clip.first + if looping { sample % clip.count } else { sample.min(clip.count - 1) }
    }
}

/// Decode one triangle-list frame from a portable model bundle.
pub fn read_frame_vertices(resources: &Resources, asset: &str, file: &str) -> Result<Vec<crate::Vertex>> {
    let bytes = resources.read_bytes(&sibling(asset, file))?;
    ensure!(!bytes.is_empty() && bytes.len() % (3 * 32) == 0, "invalid triangle data in {file}");
    bytes
        .chunks_exact(32)
        .map(|vertex| {
            let values: [f32; 8] =
                std::array::from_fn(|i| f32::from_le_bytes(vertex[i * 4..i * 4 + 4].try_into().unwrap()));
            ensure!(values.iter().all(|v| v.is_finite()), "invalid vertex in {file}");
            Ok(crate::Vertex {
                pos: [values[0], values[1], values[2]],
                normal: [values[3], values[4], values[5]],
                uv: [values[6], values[7]],
            })
        })
        .collect()
}

pub fn sibling(asset: &str, name: &str) -> String {
    match asset.rsplit_once('/') {
        Some((directory, _)) => format!("{directory}/{name}"),
        None => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_selection_respects_speed_and_looping() {
        let asset = ModelAsset {
            materials: Vec::new(),
            frames: vec![Vec::new(); 7],
            clips: vec![
                Clip { name: "Idle".into(), fps: 4.0, first: 1, count: 2 },
                Clip { name: "Run".into(), fps: 4.0, first: 3, count: 4 },
            ],
            bounds: [[0.0; 3]; 2],
            warnings: Vec::new(),
            rest: None,
        };
        assert_eq!(asset.frame("", 0.0, 1.0, true), 1);
        assert_eq!(asset.frame("Run", 0.5, 1.0, true), 5);
        assert_eq!(asset.frame("Run", 0.5, 2.0, true), 3);
        assert_eq!(asset.frame("Run", 0.5, 2.0, false), 6);
        assert_eq!(asset.frame("Run", 0.5, 0.0, false), 3);
        assert_eq!(asset.frame("Missing", 0.5, 1.0, true), 0);
    }
}
