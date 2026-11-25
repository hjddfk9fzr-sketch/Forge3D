use super::{finish, png, read_path};
use crate::{
    lighting::{Alpha, Surface},
    runtime::{ModelVertex, Part},
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Cursor, Read},
    path::Path,
};
use ufbx::{Material, MaterialMap, Texture, TextureType};

const MAX_FILE: u64 = 128 * 1024 * 1024;
const MAX_TRIANGLES: usize = 2_000_000;

pub(super) fn parts(path: &Path, root: Option<&Path>) -> Result<Vec<Part>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_FILE + 1).read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_FILE {
        return Err("FBX file exceeds 128 MiB.".into());
    }
    let options = ufbx::LoadOpts {
        file_format: ufbx::FileFormat::Fbx,
        ignore_animation: true,
        skip_skin_vertices: true,
        // Resolve textures ourselves, using the same project boundary as OBJ/glTF.
        load_external_files: false,
        target_axes: ufbx::axes_right_handed_y_up(),
        target_unit_meters: 1.,
        generate_missing_normals: true,
        node_depth_limit: 256,
        temp_allocator: ufbx::AllocatorOpts {
            memory_limit: 256 * 1024 * 1024,
            ..Default::default()
        },
        result_allocator: ufbx::AllocatorOpts {
            memory_limit: 256 * 1024 * 1024,
            ..Default::default()
        },
        ..Default::default()
    };
    let scene = ufbx::load_memory(&bytes, options)
        .map_err(|error| format!("FBX: {}", error.description))?;
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let mut parts = Vec::new();
    let mut triangle_count = 0;
    let mut corners = Vec::new();
    for node in &scene.nodes {
        let Some(mesh) = node.mesh.as_deref() else {
            continue;
        };
        triangle_count += mesh.num_triangles;
        if triangle_count > MAX_TRIANGLES {
            return Err("FBX exceeds two million triangles after expanding mesh instances.".into());
        }
        let transform = &node.geometry_to_world;
        let normal_transform = ufbx::matrix_for_normals(transform);
        let mirrored = ufbx::matrix_determinant(transform).is_sign_negative();
        let mut batches = BTreeMap::<usize, Vec<ModelVertex>>::new();
        for (face_index, face) in mesh.faces.iter().enumerate() {
            if face.num_indices < 3 || mesh.face_hole.get(face_index).copied().unwrap_or(false) {
                continue;
            }
            corners.resize((face.num_indices as usize - 2) * 3, 0);
            let written = ufbx::triangulate_face(&mut corners, mesh, *face) as usize * 3;
            let slot = mesh.face_material.get(face_index).copied().unwrap_or(0) as usize;
            let material = node
                .materials
                .get(slot)
                .or_else(|| mesh.materials.get(slot))
                .map(|value| &**value);
            let map = material.and_then(base_texture);
            let named_uv = map.and_then(|texture| {
                let name = texture.uv_set.as_ref();
                mesh.uv_sets
                    .iter()
                    .find(|set| !name.is_empty() && set.name.as_ref() == name)
            });
            let uvs = named_uv.map_or(&mesh.vertex_uv, |set| &set.vertex_uv);
            let batch = batches.entry(slot).or_default();
            for triangle in corners[..written].as_chunks::<3>().0 {
                let order = if mirrored { [0, 2, 1] } else { [0, 1, 2] };
                for offset in order {
                    let i = triangle[offset] as usize;
                    let position = ufbx::transform_position(transform, mesh.vertex_position[i]);
                    let normal = if mesh.vertex_normal.exists {
                        ufbx::transform_direction(&normal_transform, mesh.vertex_normal[i])
                    } else {
                        ufbx::Vec3::default()
                    };
                    let uv = if uvs.exists {
                        let uv = uvs[i];
                        let point = ufbx::Vec3 {
                            x: uv.x,
                            y: uv.y,
                            z: 0.,
                        };
                        let uv = map
                            .filter(|texture| texture.has_uv_transform)
                            .map_or(point, |texture| {
                                ufbx::transform_position(&texture.uv_to_texture, point)
                            });
                        [uv.x as f32, 1. - uv.y as f32]
                    } else {
                        [0.; 2]
                    };
                    let color = if mesh.vertex_color.exists {
                        rgba(mesh.vertex_color[i])
                    } else {
                        [1.; 4]
                    };
                    batch.push(ModelVertex {
                        position: xyz(position),
                        normal: xyz(normal),
                        uv,
                        color,
                    });
                }
            }
        }
        for (slot, vertices) in batches {
            if vertices.is_empty() {
                continue;
            }
            let material = node
                .materials
                .get(slot)
                .or_else(|| mesh.materials.get(slot))
                .map(|value| &**value);
            let texture = material
                .and_then(base_texture)
                .map(|texture| decode_texture(texture, directory, root))
                .transpose()?;
            let transparent = texture
                .as_ref()
                .is_some_and(|(_, transparent)| *transparent);
            let indices = (0..vertices.len() as u32).collect();
            let mut part = finish(vertices, indices, texture.map(|(data, _)| data))?;
            part.surface = surface(material);
            if transparent
                || part.surface.base_color[3] < 1.
                || part.vertices.iter().any(|vertex| vertex.color[3] < 1.)
            {
                part.surface.alpha = Alpha::Blend;
            }
            parts.push(part);
        }
    }
    Ok(parts)
}

fn xyz(value: ufbx::Vec3) -> [f32; 3] {
    [value.x as f32, value.y as f32, value.z as f32]
}

fn rgba(value: ufbx::Vec4) -> [f32; 4] {
    [value.x, value.y, value.z, value.w].map(|v| {
        if v.is_finite() {
            v.clamp(0., 1.) as f32
        } else {
            1.
        }
    })
}

fn base_texture(material: &Material) -> Option<&Texture> {
    [&material.pbr.base_color, &material.fbx.diffuse_color]
        .into_iter()
        .filter(|map| map.texture_enabled)
        .filter_map(|map| map.texture.as_deref())
        .find(|texture| texture.type_ == TextureType::File)
}

fn surface(material: Option<&Material>) -> Surface {
    let mut surface = Surface::default();
    let Some(material) = material else {
        return surface;
    };
    let scalar = |map: &MaterialMap, fallback: f32| {
        if map.has_value && map.value_vec4.x.is_finite() {
            map.value_vec4.x.clamp(0., 1.) as f32
        } else {
            fallback
        }
    };
    let map = if material.pbr.base_color.has_value {
        &material.pbr.base_color
    } else {
        &material.fbx.diffuse_color
    };
    if map.has_value
        && [map.value_vec4.x, map.value_vec4.y, map.value_vec4.z]
            .iter()
            .all(|value| value.is_finite())
    {
        surface.base_color = rgba(map.value_vec4);
        if map.value_components < 4 {
            surface.base_color[3] = 1.;
        }
    }
    surface.base_color[3] *= scalar(&material.pbr.opacity, 1.);
    surface.roughness = scalar(&material.pbr.roughness, surface.roughness);
    surface.metallic = scalar(&material.pbr.metalness, surface.metallic);
    surface.unlit = material.features.unlit.enabled;
    surface
}

fn decode_texture(
    texture: &Texture,
    directory: &Path,
    root: Option<&Path>,
) -> Result<(Vec<u8>, bool), String> {
    let video = texture.video.as_deref();
    let content = if texture.content.is_empty() {
        video.map_or(&[][..], |video| video.content.as_ref())
    } else {
        texture.content.as_ref()
    };
    let file;
    let data = if content.is_empty() {
        let name = [
            texture.relative_filename.as_ref(),
            texture.filename.as_ref(),
            texture.absolute_filename.as_ref(),
        ]
        .into_iter()
        .find(|name| !name.is_empty())
        .ok_or("FBX texture is missing its filename.")?;
        let normalized = name.replace('\\', "/");
        let path = Path::new(&normalized);
        let local = directory.join(path.file_name().ok_or("Invalid FBX texture filename.")?);
        let resolved = read_path(&directory.join(path), root)
            .ok()
            .filter(|path| path.is_file())
            .or_else(|| read_path(&local, root).ok().filter(|path| path.is_file()))
            .ok_or_else(|| {
                format!(
                    "FBX texture '{}': place the PNG/JPEG inside the model's project.",
                    name
                )
            })?;
        file = {
            let mut data = Vec::new();
            File::open(&resolved)
                .and_then(|source| source.take(MAX_FILE + 1).read_to_end(&mut data))
                .map_err(|error| format!("FBX texture {}: {error}", resolved.display()))?;
            data
        };
        file.as_slice()
    } else {
        content
    };
    if data.len() as u64 > MAX_FILE {
        return Err("FBX texture exceeds 128 MiB.".into());
    }
    let mut reader = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    if !matches!(
        reader.format(),
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg)
    ) {
        return Err("FBX base-color textures must be PNG or JPEG.".into());
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|error| format!("FBX texture: {error}"))?;
    let transparent =
        image.color().has_alpha() && image.to_rgba8().pixels().any(|pixel| pixel[3] != 255);
    Ok((png(image)?, transparent))
}
