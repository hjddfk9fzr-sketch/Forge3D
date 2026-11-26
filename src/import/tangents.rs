use crate::runtime::Part;
use macroquad::prelude::{Vec2, Vec3};
use std::collections::HashMap;

/// Accumulate tangents across shared vertices, splitting the ones that
/// participate in both mirrored and ordinary UV triangles.
pub(super) fn generate(part: &mut Part) -> Vec<[f32; 4]> {
    let mut remap = HashMap::new();
    let mut vertices = Vec::new();
    let mut influences = Vec::new();
    let mut frames = Vec::new();
    for triangle in part.indices.as_chunks_mut::<3>().0 {
        let [a, b, c] = [triangle[0], triangle[1], triangle[2]].map(|i| part.vertices[i as usize]);
        let ab = Vec3::from_array(b.position) - Vec3::from_array(a.position);
        let ac = Vec3::from_array(c.position) - Vec3::from_array(a.position);
        let uv_a = Vec2::from_array(a.uv);
        let uv_b = Vec2::from_array(b.uv) - uv_a;
        let uv_c = Vec2::from_array(c.uv) - uv_a;
        let determinant = uv_b.perp_dot(uv_c);
        let (tangent, bitangent) = if determinant.abs() > 1e-10 {
            (
                (ab * uv_c.y - ac * uv_b.y) / determinant,
                (ac * uv_b.x - ab * uv_c.x) / determinant,
            )
        } else {
            (Vec3::ZERO, Vec3::ZERO)
        };
        for index in triangle {
            let original = *index as usize;
            let vertex = part.vertices[original];
            let n = Vec3::from_array(vertex.normal);
            let mirrored = n.cross(tangent).dot(bitangent) < 0.;
            let target = *remap.entry((original, mirrored)).or_insert_with(|| {
                let target = vertices.len() as u32;
                vertices.push(vertex);
                if let Some(influence) = part.influences.get(original) {
                    influences.push(*influence);
                }
                frames.push((Vec3::ZERO, mirrored));
                target
            });
            frames[target as usize].0 += tangent;
            *index = target;
        }
    }
    part.vertices = vertices;
    part.influences = influences;
    frames
        .into_iter()
        .zip(&part.vertices)
        .map(|((t, mirrored), v)| {
            let n = Vec3::from_array(v.normal);
            let t = (t - n * n.dot(t)).normalize_or_zero();
            let t = if t == Vec3::ZERO {
                crate::runtime::tangent_reference(n)
            } else {
                t
            };
            t.extend(if mirrored { -1. } else { 1. }).to_array()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{ModelVertex, Scene};

    #[test]
    fn splits_mirrored_uvs_at_shared_vertices() {
        let model = Scene::rendering_demo().models.remove(0);
        let mut part = model.parts[0].clone();
        part.vertices = [
            ([0., 0., 0.], [0., 0.]),
            ([1., 0., 0.], [1., 0.]),
            ([0., 1., 0.], [0., 1.]),
            ([-1., 0., 0.], [1., 0.]),
        ]
        .map(|(position, uv)| ModelVertex {
            position,
            uv,
            normal: [0., 0., 1.],
            color: [1.; 4],
        })
        .to_vec();
        part.indices = vec![0, 1, 2, 0, 2, 3];
        part.influences.truncate(4);
        let tangents = generate(&mut part);
        assert_eq!(part.vertices.len(), 6);
        assert_eq!(part.influences.len(), 6);
        assert_eq!(tangents[part.indices[0] as usize], [1., 0., 0., 1.]);
        assert_eq!(tangents[part.indices[3] as usize], [-1., 0., 0., -1.]);
    }
}
