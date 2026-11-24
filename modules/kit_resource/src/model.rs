//! 3D model import: glTF 2.0 (`.gltf`, `.glb`) and Wavefront OBJ (`.obj`).
//!
//! A model is flattened into parts: one per mesh primitive (glTF) or object
//! (OBJ), with node transforms baked into the vertices so every part shares
//! the model's origin.

use anyhow::{Context, Result, bail};
use std::path::Path;

/// A model texture: a file in the project, or image bytes embedded in the model.
#[derive(Clone, Debug, PartialEq)]
pub enum TextureSource {
    /// Path relative to the project.
    File(String),
    /// Encoded image bytes (PNG/JPEG) and a file extension.
    Embedded(Vec<u8>, &'static str),
}

/// One drawable piece of a model.
#[derive(Clone, Debug, Default)]
pub struct MeshPart {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// Base color (sRGB, straight alpha).
    pub color: [f32; 4],
    pub texture: Option<TextureSource>,
}

impl MeshPart {
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in &self.positions {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        (lo, hi)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ImportedModel {
    pub parts: Vec<MeshPart>,
}

impl ImportedModel {
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in &self.parts {
            let (a, b) = p.bounds();
            for i in 0..3 {
                lo[i] = lo[i].min(a[i]);
                hi[i] = hi[i].max(b[i]);
            }
        }
        (lo, hi)
    }
}

/// File extensions Kit can import.
pub const MODEL_EXTENSIONS: &[&str] = &["glb", "gltf", "obj"];

pub fn is_model(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    MODEL_EXTENSIONS.iter().any(|e| p.ends_with(&format!(".{e}")))
}

/// Load a model from bytes. `rel` is its project-relative path (for resolving
/// external buffers/textures); `read` loads another project file.
pub fn load(rel: &str, bytes: &[u8], read: &dyn Fn(&str) -> Result<Vec<u8>>) -> Result<ImportedModel> {
    let lower = rel.to_ascii_lowercase();
    let mut model = if lower.ends_with(".glb") || lower.ends_with(".gltf") {
        load_gltf(rel, bytes, read)?
    } else if lower.ends_with(".obj") {
        load_obj(rel, bytes, read)?
    } else {
        bail!("{rel}: unsupported model format (use .glb, .gltf or .obj)");
    };
    model.parts.retain(|p| !p.indices.is_empty());
    for part in &mut model.parts {
        if part.normals.len() != part.positions.len() {
            part.normals = smooth_normals(&part.positions, &part.indices);
        }
        if part.uvs.len() != part.positions.len() {
            part.uvs = vec![[0.0, 0.0]; part.positions.len()];
        }
    }
    if model.parts.is_empty() {
        bail!("{rel}: the model has no triangles");
    }
    Ok(model)
}

fn sibling(rel: &str, name: &str) -> String {
    match rel.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{name}"),
        None => name.to_string(),
    }
}

fn load_gltf(rel: &str, bytes: &[u8], read: &dyn Fn(&str) -> Result<Vec<u8>>) -> Result<ImportedModel> {
    let gltf = gltf::Gltf::from_slice(bytes).with_context(|| format!("{rel}: invalid glTF"))?;
    // Buffers: the GLB blob, external .bin files, or data: URIs.
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    for buffer in gltf.buffers() {
        let data = match buffer.source() {
            gltf::buffer::Source::Bin => gltf.blob.clone().context("GLB has no binary chunk")?,
            gltf::buffer::Source::Uri(uri) => {
                if let Some(rest) = uri.strip_prefix("data:") {
                    let b64 = rest.split_once(";base64,").map(|(_, d)| d).context("unsupported data URI")?;
                    gltf_base64(b64)?
                } else {
                    read(&sibling(rel, &urldecode(uri)))?
                }
            }
        };
        buffers.push(data);
    }
    let get = |b: gltf::Buffer| -> Option<&[u8]> { buffers.get(b.index()).map(|v| v.as_slice()) };

    let mut parts = Vec::new();
    let scene = gltf.default_scene().or_else(|| gltf.scenes().next());
    let mut stack: Vec<(gltf::Node, [[f32; 4]; 4])> = match &scene {
        Some(s) => s.nodes().map(|n| (n, IDENTITY)).collect(),
        None => gltf.nodes().map(|n| (n, IDENTITY)).collect(),
    };
    while let Some((node, parent)) = stack.pop() {
        let m = mul(parent, node.transform().matrix());
        for child in node.children() {
            stack.push((child, m));
        }
        let Some(mesh) = node.mesh() else { continue };
        let normal_m = normal_matrix(m);
        for (pi, prim) in mesh.primitives().enumerate() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = prim.reader(|b| get(b));
            let Some(pos) = reader.read_positions() else { continue };
            let positions: Vec<[f32; 3]> = pos.map(|p| transform_point(m, p)).collect();
            let normals: Vec<[f32; 3]> =
                reader.read_normals().map(|n| n.map(|v| transform_normal(normal_m, v)).collect()).unwrap_or_default();
            let uvs: Vec<[f32; 2]> = reader.read_tex_coords(0).map(|t| t.into_f32().collect()).unwrap_or_default();
            let mut indices: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            // Mirrored transforms flip winding; keep triangles front-facing.
            if det3(m) < 0.0 {
                for t in indices.chunks_mut(3) {
                    t.swap(1, 2);
                }
            }
            let material = prim.material();
            let pbr = material.pbr_metallic_roughness();
            let c = pbr.base_color_factor();
            // glTF colors are linear; Kit stores sRGB.
            let color = [to_srgb(c[0]), to_srgb(c[1]), to_srgb(c[2]), c[3]];
            let texture = pbr.base_color_texture().and_then(|info| match info.texture().source().source() {
                gltf::image::Source::View { view, mime_type } => {
                    let data = get(view.buffer())?;
                    let bytes = data.get(view.offset()..view.offset() + view.length())?.to_vec();
                    Some(TextureSource::Embedded(bytes, if mime_type.contains("jpeg") { "jpg" } else { "png" }))
                }
                gltf::image::Source::Uri { uri, .. } => {
                    if let Some(rest) = uri.strip_prefix("data:") {
                        let ext = if rest.starts_with("image/jpeg") { "jpg" } else { "png" };
                        let b64 = rest.split_once(";base64,")?.1;
                        gltf_base64(b64).ok().map(|b| TextureSource::Embedded(b, ext))
                    } else {
                        Some(TextureSource::File(sibling(rel, &urldecode(uri))))
                    }
                }
            });
            let name = match (node.name(), mesh.primitives().len()) {
                (Some(n), 1) => n.to_string(),
                (Some(n), _) => format!("{n} {}", pi + 1),
                (None, _) => mesh.name().map(String::from).unwrap_or_else(|| format!("Part {}", parts.len() + 1)),
            };
            parts.push(MeshPart { name, positions, normals, uvs, indices, color, texture });
        }
    }
    Ok(ImportedModel { parts })
}

fn load_obj(rel: &str, bytes: &[u8], read: &dyn Fn(&str) -> Result<Vec<u8>>) -> Result<ImportedModel> {
    let opts = tobj::LoadOptions { triangulate: true, single_index: true, ..Default::default() };
    let (models, materials) = tobj::load_obj_buf(&mut std::io::Cursor::new(bytes), &opts, |mtl| {
        let name = mtl.to_string_lossy().to_string();
        match read(&sibling(rel, &name)) {
            Ok(b) => tobj::load_mtl_buf(&mut std::io::Cursor::new(b)),
            Err(_) => Err(tobj::LoadError::OpenFileFailed),
        }
    })
    .with_context(|| format!("{rel}: invalid OBJ"))?;
    let materials = materials.unwrap_or_default();
    let parts = models
        .into_iter()
        .enumerate()
        .map(|(i, m)| {
            let mesh = m.mesh;
            let positions = mesh.positions.chunks(3).map(|p| [p[0], p[1], p[2]]).collect();
            let normals = mesh.normals.chunks(3).map(|n| [n[0], n[1], n[2]]).collect();
            // OBJ texture v points up; Kit's v points down.
            let uvs = mesh.texcoords.chunks(2).map(|t| [t[0], 1.0 - t[1]]).collect();
            let mat = mesh.material_id.and_then(|id| materials.get(id));
            let d = mat.and_then(|m| m.diffuse).unwrap_or([0.8, 0.8, 0.8]);
            let texture = mat
                .and_then(|m| m.diffuse_texture.clone())
                .map(|t| TextureSource::File(sibling(rel, &t.replace('\\', "/"))));
            MeshPart {
                name: if m.name.is_empty() { format!("Part {}", i + 1) } else { m.name },
                positions,
                normals,
                uvs,
                indices: mesh.indices,
                color: [d[0], d[1], d[2], mat.and_then(|m| m.dissolve).unwrap_or(1.0)],
                texture,
            }
        })
        .collect();
    Ok(ImportedModel { parts })
}

/// Area-weighted vertex normals.
pub fn smooth_normals(pos: &[[f32; 3]], idx: &[u32]) -> Vec<[f32; 3]> {
    let mut n = vec![[0.0f32; 3]; pos.len()];
    for t in idx.chunks(3) {
        if t.len() < 3 {
            continue;
        }
        let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let f = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for &i in t {
            for k in 0..3 {
                n[i as usize][k] += f[k];
            }
        }
    }
    for v in &mut n {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        *v = if l > 1e-12 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0, 1.0, 0.0] };
    }
    n
}

const IDENTITY: [[f32; 4]; 4] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// Column-major 4x4 multiply (a * b).
fn mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut o = [[0.0; 4]; 4];
    for c in 0..4 {
        for r in 0..4 {
            o[c][r] = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    o
}

fn transform_point(m: [[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    let mut o = [0.0; 3];
    for (r, out) in o.iter_mut().enumerate() {
        *out = m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r];
    }
    o
}

fn det3(m: [[f32; 4]; 4]) -> f32 {
    m[0][0] * (m[1][1] * m[2][2] - m[2][1] * m[1][2]) - m[1][0] * (m[0][1] * m[2][2] - m[2][1] * m[0][2])
        + m[2][0] * (m[0][1] * m[1][2] - m[1][1] * m[0][2])
}

/// Inverse-transpose of the upper 3x3 (for normals), row-major result.
fn normal_matrix(m: [[f32; 4]; 4]) -> [[f32; 3]; 3] {
    let a = |c: usize, r: usize| m[c][r];
    let det = det3(m);
    let inv_det = if det.abs() > 1e-12 { 1.0 / det } else { 1.0 };
    // Cofactor matrix = (inverse)^T * det.
    let mut n = [[0.0f32; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
            let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
            n[r][c] = (a(c1, r1) * a(c2, r2) - a(c2, r1) * a(c1, r2)) * inv_det;
        }
    }
    n
}

fn transform_normal(n: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    let o = [
        n[0][0] * v[0] + n[0][1] * v[1] + n[0][2] * v[2],
        n[1][0] * v[0] + n[1][1] * v[1] + n[1][2] * v[2],
        n[2][0] * v[0] + n[2][1] * v[1] + n[2][2] * v[2],
    ];
    let l = (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt().max(1e-12);
    [o[0] / l, o[1] / l, o[2] / l]
}

fn to_srgb(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

fn urldecode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn gltf_base64(s: &str) -> Result<Vec<u8>> {
    // Minimal standard base64 decoder (data: URIs in .gltf files).
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => bail!("invalid base64 in data URI"),
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Ok(out)
}

/// Read a model file from disk (used by tests and tools).
pub fn load_file(path: &Path) -> Result<ImportedModel> {
    let bytes = std::fs::read(path)?;
    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    load(&name, &bytes, &|rel| Ok(std::fs::read(dir.join(rel))?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn obj_with_material() {
        let obj = "mtllib m.mtl\no Box\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nusemtl red\nf 1 2 3 4\n";
        let mtl = "newmtl red\nKd 1 0 0\n";
        let m = load("art/box.obj", obj.as_bytes(), &|p| {
            assert_eq!(p, "art/m.mtl");
            Ok(mtl.as_bytes().to_vec())
        })
        .unwrap();
        assert_eq!(m.parts.len(), 1);
        let p = &m.parts[0];
        assert_eq!(p.name, "Box");
        assert_eq!(p.indices.len(), 6, "quad triangulated");
        assert_eq!(p.color, [1.0, 0.0, 0.0, 1.0]);
        assert!((p.normals[0][2] - 1.0).abs() < 1e-5, "generated normal faces +Z");
    }

    #[test]
    fn gltf_embedded_buffer_and_node_transform() {
        // One triangle, translated by its node to x + 5.
        let mut bin = Vec::new();
        for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for c in v {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bin);
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
            "nodes":[{{"name":"Tri","mesh":0,"translation":[5,0,0]}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"material":0}}]}}],
            "materials":[{{"pbrMetallicRoughness":{{"baseColorFactor":[0.2140,1,1,1]}}}}],
            "buffers":[{{"byteLength":36,"uri":"data:application/octet-stream;base64,{b64}"}}],
            "bufferViews":[{{"buffer":0,"byteLength":36}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}}]}}"#
        );
        let m = load("t.gltf", json.as_bytes(), &|_| bail!("no files")).unwrap();
        assert_eq!(m.parts.len(), 1);
        let p = &m.parts[0];
        assert_eq!(p.name, "Tri");
        assert_eq!(p.positions[1], [6.0, 0.0, 0.0]);
        assert!((p.color[0] - 0.5).abs() < 0.01, "linear 0.214 -> sRGB 0.5");
        let (lo, hi) = m.bounds();
        assert_eq!((lo[0], hi[0]), (5.0, 6.0));
    }

    #[test]
    fn real_glb_if_present() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello/models/dimension-logo.glb");
        if p.exists() {
            let m = load_file(&p).unwrap();
            assert!(!m.parts.is_empty());
        }
    }
}
