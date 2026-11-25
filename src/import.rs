mod fbx;

use crate::{
    animation::{Clip, Influence, Interpolation, Node, Property, Rig, Skin, Track},
    lighting::{Alpha, Surface},
    runtime::{Model, ModelVertex, Part},
};
use image::{DynamicImage, ImageFormat, RgbaImage};
use macroquad::prelude::{Mat4, Vec3};
use std::path::Path;
use std::{
    fs::File,
    io::{BufReader, Cursor},
};

pub fn model(path: &Path) -> Result<Model, String> {
    import_model(path, None)
}

pub fn model_under(path: &Path, root: &Path) -> Result<Model, String> {
    import_model(&read_path(path, Some(root))?, Some(root))
}

fn read_path(path: &Path, root: Option<&Path>) -> Result<std::path::PathBuf, String> {
    let Some(root) = root else {
        return Ok(path.to_owned());
    };
    let path = path.canonicalize().map_err(|error| error.to_string())?;
    if !path.starts_with(root) || !path.is_file() {
        return Err("The model and its dependencies must be files inside the project root.".into());
    }
    Ok(path)
}

fn import_model(path: &Path, root: Option<&Path>) -> Result<Model, String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let (parts, rig) = match extension.to_ascii_lowercase().as_str() {
        "obj" => obj(path, root).map(|parts| (parts, None)),
        "fbx" => fbx::parts(path, root).map(|parts| (parts, None)),
        "gltf" | "glb" => gltf_parts(path, root),
        _ => return Err("Choose an OBJ, FBX, glTF or GLB file.".into()),
    }?;
    if parts.is_empty() {
        return Err("The file has no supported triangle meshes.".into());
    }
    Ok(Model {
        name: path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        parts: parts.into(),
        rig: rig.map(std::sync::Arc::new),
    })
}

fn obj(path: &Path, root: Option<&Path>) -> Result<Vec<Part>, String> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let file = File::open(path).map_err(|error| error.to_string())?;
    let (objects, materials) = tobj::load_obj_buf(
        &mut BufReader::new(file),
        &tobj::GPU_LOAD_OPTIONS,
        |material| {
            let path = read_path(&directory.join(material), root)
                .map_err(|_| tobj::LoadError::OpenFileFailed)?;
            tobj::load_mtl(path)
        },
    )
    .map_err(|error| error.to_string())?;
    let materials = materials.map_err(|error| format!("OBJ materials: {error}"))?;
    objects
        .into_iter()
        .filter(|object| !object.mesh.indices.is_empty())
        .map(|object| {
            let mesh = object.mesh;
            let material = mesh.material_id.and_then(|index| materials.get(index));
            let [r, g, b] = material.and_then(|value| value.diffuse).unwrap_or([0.8; 3]);
            let texture = material
                .and_then(|value| value.diffuse_texture.as_deref())
                .filter(|name| !name.is_empty())
                .map(|name| {
                    let image = image::open(read_path(&directory.join(name), root)?)
                        .map_err(|error| format!("{name}: {error}"))?;
                    let transparent = image.color().has_alpha()
                        && image.to_rgba8().pixels().any(|pixel| pixel[3] != 255);
                    Ok::<_, String>((png(image)?, transparent))
                })
                .transpose()?;
            let vertices = mesh
                .positions
                .as_chunks::<3>()
                .0
                .iter()
                .enumerate()
                .map(|(index, p)| {
                    let uv = mesh
                        .texcoords
                        .get(index * 2..index * 2 + 2)
                        .map_or([0.; 2], |uv| [uv[0], 1. - uv[1]]);
                    let normal = mesh
                        .normals
                        .get(index * 3..index * 3 + 3)
                        .map_or([0.; 3], |n| [n[0], n[1], n[2]]);
                    ModelVertex {
                        position: [p[0], p[1], p[2]],
                        normal,
                        uv,
                        color: [r, g, b, 1.],
                    }
                })
                .collect();
            let (texture, transparent) = texture.map_or((None, false), |(texture, transparent)| {
                (Some(texture), transparent)
            });
            let mut part = finish(vertices, mesh.indices, texture)?;
            if transparent {
                part.surface.alpha = Alpha::Blend;
            }
            Ok(part)
        })
        .collect()
}

fn gltf_parts(path: &Path, root: Option<&Path>) -> Result<(Vec<Part>, Option<Rig>), String> {
    use gltf::{buffer, image};
    let gltf::Gltf { document, blob } =
        gltf::Gltf::open(path).map_err(|error| error.to_string())?;
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let uris = document
        .buffers()
        .filter_map(|buffer| match buffer.source() {
            buffer::Source::Uri(uri) => Some(uri),
            _ => None,
        })
        .chain(document.images().filter_map(|image| match image.source() {
            image::Source::Uri { uri, .. } => Some(uri),
            _ => None,
        }));
    for uri in uris {
        if uri.starts_with("data:") {
            continue;
        }
        let relative = urlencoding::decode(uri).map_err(|error| error.to_string())?;
        if root.is_some() {
            if uri.contains(':') || Path::new(relative.as_ref()).is_absolute() {
                return Err("Use a relative glTF resource path inside the project root.".into());
            }
            read_path(&directory.join(relative.as_ref()), root)?;
        }
    }
    let buffers = gltf::import_buffers(&document, Some(directory), blob)
        .map_err(|error| error.to_string())?;
    let images = gltf::import_images(&document, Some(directory), &buffers)
        .map_err(|error| error.to_string())?;
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .ok_or("glTF has no scene.")?;
    let rig = gltf_rig(&document, &scene, &buffers)?;
    let mut parts = Vec::new();
    let mut pending: Vec<_> = scene.nodes().map(|node| (node, Mat4::IDENTITY)).collect();
    pending.reverse();
    let mut visited = std::collections::HashSet::new();
    while let Some((node, parent)) = pending.pop() {
        if !visited.insert(node.index()) {
            return Err("glTF contains a repeated node or cycle.".into());
        }
        let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        gltf_node(
            &node,
            transform,
            &buffers,
            &images,
            rig.is_some(),
            &mut parts,
        )?;
        let mut children: Vec<_> = node.children().collect();
        pending.extend(children.drain(..).rev().map(|child| (child, transform)));
    }
    Ok((parts, rig))
}

fn gltf_rig(
    document: &gltf::Document,
    scene: &gltf::Scene<'_>,
    buffers: &[gltf::buffer::Data],
) -> Result<Option<Rig>, String> {
    use gltf::animation::{util::ReadOutputs, Interpolation as GltfInterpolation};
    if document.animations().len() == 0 && document.skins().len() == 0 {
        return Ok(None);
    }
    let mut nodes: Vec<_> = document
        .nodes()
        .map(|node| {
            let (position, rotation, scale) = node.transform().decomposed();
            Node {
                parent: None,
                position,
                rotation,
                scale,
            }
        })
        .collect();
    for node in document.nodes() {
        for child in node.children() {
            if nodes[child.index()].parent.replace(node.index()).is_some() {
                return Err("glTF nodes have more than one parent.".into());
            }
        }
    }
    for root in scene.nodes() {
        nodes[root.index()].parent = None;
    }
    let skins = document
        .skins()
        .map(|skin| {
            let joints: Vec<_> = skin.joints().map(|node| node.index()).collect();
            let reader = skin.reader(|buffer| Some(&buffers[buffer.index()].0));
            let inverse_bind = reader.read_inverse_bind_matrices().map_or_else(
                || vec![Mat4::IDENTITY.to_cols_array_2d(); joints.len()],
                Iterator::collect,
            );
            Skin {
                joints,
                inverse_bind,
            }
        })
        .collect();
    let mut clips: Vec<Clip> = Vec::new();
    for animation in document.animations() {
        let base = animation
            .name()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Animation {}", animation.index() + 1));
        let mut name = base.clone();
        let mut suffix = 2;
        while clips.iter().any(|clip| clip.name == name) {
            name = format!("{base} ({suffix})");
            suffix += 1;
        }
        let mut duration = 0_f32;
        let mut tracks = Vec::new();
        for channel in animation.channels() {
            let reader = channel.reader(|buffer| Some(&buffers[buffer.index()].0));
            let values3 = |values: gltf::accessor::Iter<'_, [f32; 3]>| {
                values.map(|[x, y, z]| [x, y, z, 0.]).collect::<Vec<_>>()
            };
            let (property, values) = match reader
                .read_outputs()
                .ok_or("Missing glTF animation output.")?
            {
                ReadOutputs::Translations(values) => (Property::Translation, values3(values)),
                ReadOutputs::Rotations(values) => (Property::Rotation, values.into_f32().collect()),
                ReadOutputs::Scales(values) => (Property::Scale, values3(values)),
                ReadOutputs::MorphTargetWeights(_) => continue,
            };
            let times: Vec<_> = reader
                .read_inputs()
                .ok_or("Missing glTF animation timestamps.")?
                .collect();
            if let Some(&last) = times.last() {
                duration = duration.max(last);
            }
            let interpolation = match channel.sampler().interpolation() {
                GltfInterpolation::Step => Interpolation::Step,
                GltfInterpolation::Linear => Interpolation::Linear,
                GltfInterpolation::CubicSpline => Interpolation::Cubic,
            };
            tracks.push(Track {
                node: channel.target().node().index(),
                property,
                interpolation,
                times,
                values,
            });
        }
        if !tracks.is_empty() {
            clips.push(Clip {
                name,
                duration,
                tracks,
            });
        }
    }
    let rig = Rig {
        nodes,
        skins,
        clips,
    };
    if !rig.validate() {
        return Err("glTF contains an invalid skin or animation track.".into());
    }
    Ok(Some(rig))
}

fn gltf_node(
    node: &gltf::Node<'_>,
    transform: Mat4,
    buffers: &[gltf::buffer::Data],
    images: &[gltf::image::Data],
    animated: bool,
    parts: &mut Vec<Part>,
) -> Result<(), String> {
    let local = if animated { Mat4::IDENTITY } else { transform };
    let normal_transform = local.inverse().transpose();
    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()].0));
            let positions = reader.read_positions().ok_or("Missing glTF positions.")?;
            let count = positions.len();
            let indices = reader.read_indices().map_or_else(
                || (0..count as u32).collect(),
                |indices| indices.into_u32().collect(),
            );
            let material = primitive.material();
            let pbr = material.pbr_metallic_roughness();
            let base = pbr.base_color_texture();
            let metal = pbr.metallic_roughness_texture();
            let emission = material.emissive_texture();
            let uv_set = base
                .as_ref()
                .or(metal.as_ref())
                .or(emission.as_ref())
                .map_or(0, |info| info.tex_coord());
            let mut uv = reader.read_tex_coords(uv_set).map(|value| value.into_f32());
            let mut color = reader.read_colors(0).map(|value| value.into_rgba_f32());
            let mut normals = reader.read_normals();
            let vertices = positions
                .map(|position| {
                    let normal =
                        normals
                            .as_mut()
                            .and_then(Iterator::next)
                            .map_or(Vec3::ZERO, |value| {
                                normal_transform
                                    .transform_vector3(Vec3::from_array(value))
                                    .normalize_or_zero()
                            });
                    ModelVertex {
                        position: local
                            .transform_point3(Vec3::from_array(position))
                            .to_array(),
                        normal: normal.to_array(),
                        uv: uv.as_mut().and_then(Iterator::next).unwrap_or([0.; 2]),
                        color: color.as_mut().and_then(Iterator::next).unwrap_or([1.; 4]),
                    }
                })
                .collect();
            let texture = |info: Option<gltf::texture::Info<'_>>| {
                info.filter(|info| info.tex_coord() == uv_set)
                    .map(|info| gltf_png(&images[info.texture().source().index()]))
                    .transpose()
            };
            let mut part = finish(vertices, indices, texture(base)?)?;
            part.metallic_roughness = texture(metal)?;
            part.emissive_texture = texture(emission)?;
            part.surface = Surface {
                base_color: pbr.base_color_factor(),
                metallic: pbr.metallic_factor(),
                roughness: pbr.roughness_factor(),
                emissive: material.emissive_factor(),
                use_color_shader: false,
                unlit: material.unlit(),
                alpha_cutoff: material.alpha_cutoff().unwrap_or(0.5),
                alpha: match material.alpha_mode() {
                    gltf::material::AlphaMode::Opaque => Alpha::Opaque,
                    gltf::material::AlphaMode::Mask => Alpha::Mask,
                    gltf::material::AlphaMode::Blend => Alpha::Blend,
                },
            };
            part.node = animated.then(|| node.index());
            if let Some(skin) = node.skin() {
                let joints = reader
                    .read_joints(0)
                    .ok_or("Skinned glTF primitive needs JOINTS_0.")?
                    .into_u16();
                let weights = reader
                    .read_weights(0)
                    .ok_or("Skinned glTF primitive needs WEIGHTS_0.")?
                    .into_f32();
                part.influences = joints
                    .zip(weights)
                    .map(|(joints, weights)| Influence { joints, weights })
                    .collect();
                let joint_count = skin.joints().len();
                if part.influences.len() != count
                    || part.influences.iter().any(|vertex| {
                        vertex
                            .joints
                            .into_iter()
                            .zip(vertex.weights)
                            .any(|(index, weight)| {
                                !weight.is_finite()
                                    || weight < 0.
                                    || (weight > 0. && usize::from(index) >= joint_count)
                            })
                    })
                {
                    return Err("Invalid glTF skin influences.".into());
                }
                part.skin = Some(skin.index());
            }
            parts.push(part);
        }
    }
    Ok(())
}

fn finish(
    mut vertices: Vec<ModelVertex>,
    indices: Vec<u32>,
    texture: Option<Vec<u8>>,
) -> Result<Part, String> {
    if indices.is_empty()
        || !indices.len().is_multiple_of(3)
        || indices.iter().any(|&i| i as usize >= vertices.len())
    {
        return Err("Invalid triangle indices.".into());
    }
    if vertices
        .iter()
        .any(|v| !Vec3::from_array(v.position).is_finite())
    {
        return Err("Invalid model positions.".into());
    }
    let mut accumulated = vec![Vec3::ZERO; vertices.len()];
    for triangle in indices.as_chunks::<3>().0 {
        let [a, b, c] =
            std::array::from_fn(|i| Vec3::from_array(vertices[triangle[i] as usize].position));
        let normal = (b - a).cross(c - a);
        for &index in triangle {
            accumulated[index as usize] += normal;
        }
    }
    for (vertex, normal) in vertices.iter_mut().zip(accumulated) {
        let existing = Vec3::from_array(vertex.normal).normalize_or_zero();
        vertex.normal = if existing == Vec3::ZERO {
            normal.normalize_or_zero()
        } else {
            existing
        }
        .to_array();
    }
    Ok(Part {
        surface: Surface::default(),
        metallic_roughness: None,
        emissive_texture: None,
        node: None,
        skin: None,
        influences: Vec::new(),
        vertices,
        indices,
        texture,
    })
}

fn gltf_png(data: &gltf::image::Data) -> Result<Vec<u8>, String> {
    use gltf::image::Format::*;
    let (channels, bytes) = match data.format {
        R8 => (1, 1),
        R8G8 => (2, 1),
        R8G8B8 => (3, 1),
        R8G8B8A8 => (4, 1),
        R16 => (1, 2),
        R16G16 => (2, 2),
        R16G16B16 => (3, 2),
        R16G16B16A16 => (4, 2),
        R32G32B32FLOAT => (3, 4),
        R32G32B32A32FLOAT => (4, 4),
    };
    let rgba = data
        .pixels
        .chunks_exact(channels * bytes)
        .flat_map(|pixel| {
            let component = |index| -> u8 {
                let offset = index * bytes;
                match bytes {
                    1 => pixel[offset],
                    2 => (u16::from_ne_bytes([pixel[offset], pixel[offset + 1]]) >> 8) as u8,
                    _ => {
                        (f32::from_ne_bytes(pixel[offset..offset + 4].try_into().unwrap())
                            .clamp(0., 1.)
                            * 255.) as u8
                    }
                }
            };
            match channels {
                1 => [component(0), component(0), component(0), 255],
                2 => [component(0), component(0), component(0), component(1)],
                3 => [component(0), component(1), component(2), 255],
                _ => [component(0), component(1), component(2), component(3)],
            }
        })
        .collect();
    let image = RgbaImage::from_raw(data.width, data.height, rgba).ok_or("Invalid glTF image.")?;
    png(DynamicImage::ImageRgba8(image))
}

fn png(image: DynamicImage) -> Result<Vec<u8>, String> {
    let mut buffer = Cursor::new(Vec::new());
    image
        .write_to(&mut buffer, ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(buffer.into_inner())
}
