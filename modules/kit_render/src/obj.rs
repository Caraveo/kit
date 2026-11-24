//! Wavefront OBJ geometry. Kit's Model component supplies the material.

use crate::{MeshData, Vertex};
use anyhow::{Context, Result, ensure};
use glam::{Vec2, Vec3};

pub fn load_obj(path: &std::path::Path) -> Result<MeshData> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_obj(&text).with_context(|| format!("loading {}", path.display()))
}

/// Parse OBJ text.
pub fn parse_obj(text: &str) -> Result<MeshData> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut mesh = MeshData::default();
    for (line, text) in text.lines().enumerate() {
        let mut fields = text.split('#').next().unwrap_or_default().split_whitespace();
        let result = match fields.next() {
            Some("v") => coordinates(&mut fields).map(|v: [f32; 3]| positions.push(Vec3::from_array(v))),
            Some("vn") => coordinates(&mut fields).map(|v: [f32; 3]| normals.push(Vec3::from_array(v))),
            Some("vt") => coordinates(&mut fields).map(|v: [f32; 2]| uvs.push(Vec2::from_array(v))),
            Some("f") => face(fields, &positions, &normals, &uvs, &mut mesh),
            _ => Ok(()),
        };
        result.with_context(|| format!("OBJ line {}", line + 1))?;
    }
    ensure!(!mesh.indices.is_empty(), "OBJ contains no non-degenerate faces");
    Ok(mesh)
}

fn coordinates<const N: usize>(fields: &mut std::str::SplitWhitespace<'_>) -> Result<[f32; N]> {
    let mut values = [0.0_f32; N];
    for value in &mut values {
        *value = fields.next().context("missing coordinate")?.parse()?;
        ensure!(value.is_finite(), "non-finite coordinate");
    }
    Ok(values)
}

fn index<T: Copy>(values: &[T], field: &str) -> Result<T> {
    let raw: i64 = field.parse().context("invalid face index")?;
    let i = if raw < 0 { values.len() as i64 + raw } else { raw - 1 };
    usize::try_from(i).ok().and_then(|i| values.get(i).copied()).context("face index out of range")
}

fn face(
    fields: std::str::SplitWhitespace<'_>,
    positions: &[Vec3],
    normals: &[Vec3],
    uvs: &[Vec2],
    mesh: &mut MeshData,
) -> Result<()> {
    let corners = fields
        .map(|field| {
            let mut parts = field.split('/');
            let position = index(positions, parts.next().unwrap_or_default())?;
            let uv = parts.next().filter(|s| !s.is_empty()).map(|s| index(uvs, s)).transpose()?;
            let normal = parts.next().filter(|s| !s.is_empty()).map(|s| index(normals, s)).transpose()?;
            ensure!(parts.next().is_none(), "invalid face corner");
            Ok((position, uv, normal))
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(corners.len() >= 3, "face needs at least three corners");
    for edge in corners[1..].windows(2) {
        let triangle = [corners[0], edge[0], edge[1]];
        let normal = (triangle[1].0 - triangle[0].0).cross(triangle[2].0 - triangle[0].0).normalize_or_zero();
        if normal == Vec3::ZERO {
            continue;
        }
        for (position, uv, supplied_normal) in triangle {
            let uv = uv.map_or(Vec2::ZERO, |uv| Vec2::new(uv.x, 1.0 - uv.y));
            mesh.indices.push(u32::try_from(mesh.vertices.len())?);
            mesh.vertices.push(Vertex {
                pos: position.to_array(),
                normal: supplied_normal.and_then(Vec3::try_normalize).unwrap_or(normal).to_array(),
                uv: uv.to_array(),
            });
        }
    }
    Ok(())
}
