//! Rectangular text maps turn occupied cells into one static mesh.
use crate::{
    lighting::Surface,
    runtime::{ModelVertex, Part},
};
use macroquad::prelude::Vec3;
use std::{fs::File, io::Read, path::Path};

const SIDE: usize = 64;
const MAX_BYTES: usize = SIDE * (SIDE + 2);

pub(super) fn parts(path: &Path) -> Result<Vec<Part>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err(format!("Grid exceeds {MAX_BYTES} bytes."));
    }
    let source = std::str::from_utf8(&bytes).map_err(|error| {
        let prefix = &bytes[..error.valid_up_to()];
        let row = prefix.iter().filter(|&&b| b == b'\n').count() + 1;
        let col = prefix
            .rsplit(|&b| b == b'\n')
            .next()
            .unwrap_or_default()
            .len()
            + 1;
        at(row, col, "expected UTF-8")
    })?;
    let rows: Vec<_> = source.lines().map(str::as_bytes).collect();
    let width = rows.first().map_or(0, |row| row.len());
    if width == 0 {
        return Err(at(1, 1, "expected a row of # or ."));
    }
    for (row, line) in rows.iter().enumerate() {
        if row == SIDE {
            return Err(at(row + 1, 1, "at most 64 rows"));
        }
        for (col, cell) in line.iter().enumerate() {
            if col == SIDE {
                return Err(at(row + 1, col + 1, "at most 64 columns"));
            }
            if !matches!(cell, b'#' | b'.') {
                return Err(at(row + 1, col + 1, "expected # or ."));
            }
        }
        if line.len() != width {
            return Err(at(
                row + 1,
                line.len().min(width) + 1,
                "rows must have the same width",
            ));
        }
    }

    let mut mesh = Part {
        surface: Surface {
            base_color: [0.38, 0.72, 0.62, 1.],
            ..Default::default()
        },
        metallic_roughness: None,
        emissive_texture: None,
        node: None,
        skin: None,
        influences: Vec::new(),
        vertices: Vec::new(),
        indices: Vec::new(),
        texture: None,
    };
    let occupied = |x: isize, z: isize| {
        rows.get(z as usize).and_then(|row| row.get(x as usize)) == Some(&b'#')
    };
    for (z, row) in rows.iter().enumerate() {
        for (x, &cell) in row.iter().enumerate() {
            if cell != b'#' {
                continue;
            }
            let center = Vec3::new(x as f32 + 0.5, -0.25, z as f32 + 0.5);
            for normal in [
                Vec3::Y,
                Vec3::NEG_Y,
                Vec3::X,
                Vec3::NEG_X,
                Vec3::Z,
                Vec3::NEG_Z,
            ] {
                if normal.y == 0.
                    && occupied(
                        x as isize + normal.x as isize,
                        z as isize + normal.z as isize,
                    )
                {
                    continue;
                }
                let reference = if normal.y == 0. { Vec3::Y } else { Vec3::Z };
                let horizontal = reference.cross(normal);
                let vertical = normal.cross(horizontal);
                let start = mesh.vertices.len() as u32;
                for (u, v) in [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)] {
                    let offset =
                        (normal + horizontal * u + vertical * v) * Vec3::new(0.5, 0.25, 0.5);
                    mesh.vertices.push(ModelVertex {
                        position: (center + offset).to_array(),
                        normal: normal.to_array(),
                        uv: [(u + 1.) * 0.5, (v + 1.) * 0.5],
                        color: [1.; 4],
                    });
                }
                mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|i| start + i));
            }
        }
    }
    if mesh.vertices.is_empty() {
        return Err(at(1, 1, "expected at least one platform (#)"));
    }
    Ok(vec![mesh])
}

fn at(row: usize, col: usize, message: &str) -> String {
    format!("Grid line {row}, column {col}: {message}.")
}
