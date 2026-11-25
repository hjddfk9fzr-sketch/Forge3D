//! Software scene preview for project tools running without an OpenGL context.
use crate::runtime::{self, Scene, Shape};
use image::{DynamicImage, ImageFormat, ImageReader, Rgb, RgbImage, RgbaImage};
use macroquad::prelude::{vec3, vec4, Mat4, Mesh, Vec2, Vec3, Vec4};
use std::{collections::HashMap, io::Cursor};

#[derive(Clone, Copy)]
struct Vertex {
    clip: Vec4,
    color: Vec4,
    uv: Vec2,
}

pub fn png(scene: &Scene, width: u32, height: u32) -> Result<Vec<u8>, String> {
    if !(64..=1024).contains(&width) || !(64..=1024).contains(&height) {
        return Err("Capture dimensions must be between 64 and 1024 pixels.".into());
    }
    scene.validate()?;
    let mut canvas = Canvas {
        image: RgbImage::from_pixel(width, height, Rgb([22, 28, 42])),
        depth: vec![f32::INFINITY; (width * height) as usize],
        budget: 48_000_000,
    };
    let camera = scene.camera.camera();
    let projection = Mat4::perspective_rh_gl(
        camera.fovy,
        width as f32 / height as f32,
        camera.z_near,
        camera.z_far,
    ) * Mat4::look_at_rh(camera.position, camera.target, camera.up);
    let cube = runtime::cube_mesh();
    let sphere = runtime::sphere_mesh();
    let mut textures = HashMap::new();
    let mut texture_bytes = 0_u64;
    for (index, (matrix, visible)) in scene.world_transforms().into_iter().enumerate() {
        if !visible {
            continue;
        }
        let mvp = projection * matrix;
        let object = &scene.objects[index];
        let light = &scene.lighting;
        let normals = if matrix.determinant().abs() > 1e-10 {
            matrix.inverse().transpose()
        } else {
            Mat4::IDENTITY
        };
        let default = crate::lighting::Surface::default();
        let material = object.surface.as_ref().unwrap_or(&default);
        let paint = |vertex_color: Vec4, normal: Vec3, material: &crate::lighting::Surface| {
            let base = vertex_color * Vec4::from_array(material.base_color);
            let normal = normals.transform_vector3(normal).normalize_or_zero();
            let ambient = if material.unlit {
                Vec3::ONE
            } else {
                Vec3::splat(light.ambient)
                    + Vec3::from_array(light.color)
                        * light.intensity
                        * normal.dot(light.sun()).max(0.)
                        * 0.27
            };
            let rgb = (base.truncate() * ambient + Vec3::from_array(material.emissive))
                .clamp(Vec3::ZERO, Vec3::ONE);
            rgb.extend(base.w)
        };
        match object.shape {
            Shape::Empty => {}
            Shape::Cube => {
                canvas.mesh(&cube, mvp, |color, normal| paint(color, normal, material))?
            }
            Shape::Sphere => {
                canvas.mesh(&sphere, mvp, |color, normal| paint(color, normal, material))?
            }
            Shape::Plane => {
                let point = |x, z| Vertex {
                    clip: mvp * vec4(x, 0., z, 1.),
                    color: paint(color([45, 55, 77, 255]), Vec3::Y, material),
                    uv: Vec2::ZERO,
                };
                let [a, b, c, d] = [
                    point(-0.5, -0.5),
                    point(0.5, -0.5),
                    point(0.5, 0.5),
                    point(-0.5, 0.5),
                ];
                canvas.triangle([a, b, c], None)?;
                canvas.triangle([a, c, d], None)?;
            }
            Shape::Model(index) => {
                let model = &scene.models[index];
                let pose = model
                    .rig
                    .as_ref()
                    .map(|rig| rig.pose(object.animation.as_ref()));
                for (part_index, part) in model.parts.iter().enumerate() {
                    let material = object.surface.as_ref().unwrap_or(&part.surface);
                    let texture = if let Some(bytes) = &part.texture {
                        let texture = match textures.entry((index, part_index)) {
                            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                            std::collections::hash_map::Entry::Vacant(entry) => {
                                let reader = || {
                                    ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png)
                                };
                                let (width, height) = reader()
                                    .into_dimensions()
                                    .map_err(|error| error.to_string())?;
                                texture_bytes = texture_bytes.saturating_add(
                                    (u64::from(width) * u64::from(height)).saturating_mul(4),
                                );
                                if texture_bytes > 128 * 1024 * 1024 {
                                    return Err(
                                        "Visible textures exceed the 128 MiB preview budget."
                                            .into(),
                                    );
                                }
                                entry.insert(
                                    reader()
                                        .decode()
                                        .map_err(|error| error.to_string())?
                                        .into_rgba8(),
                                )
                            }
                        };
                        Some(&*texture)
                    } else {
                        None
                    };
                    for face in part.indices.chunks_exact(3) {
                        let triangle = [face[0], face[1], face[2]].map(|index| {
                            let index = index as usize;
                            let vertex = pose.as_ref().map_or_else(
                                || part.vertices[index],
                                |pose| pose.vertex(part, index),
                            );
                            Vertex {
                                clip: mvp * Vec3::from_array(vertex.position).extend(1.),
                                color: paint(
                                    Vec4::from_array(vertex.color),
                                    Vec3::from_array(vertex.normal),
                                    material,
                                ),
                                uv: Vec2::from_array(vertex.uv),
                            }
                        });
                        canvas.triangle(triangle, texture)?;
                    }
                }
            }
        }
    }
    let mut data = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(canvas.image)
        .write_to(&mut data, ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(data.into_inner())
}

fn color(channels: [u8; 4]) -> Vec4 {
    Vec4::from_array(channels.map(|component| f32::from(component) / 255.))
}

struct Canvas {
    image: RgbImage,
    depth: Vec<f32>,
    budget: u64,
}

impl Canvas {
    fn mesh(
        &mut self,
        mesh: &Mesh,
        mvp: Mat4,
        paint: impl Fn(Vec4, Vec3) -> Vec4,
    ) -> Result<(), String> {
        for face in mesh.indices.chunks_exact(3) {
            let triangle = [face[0], face[1], face[2]].map(|index| {
                let vertex = mesh.vertices[usize::from(index)];
                Vertex {
                    clip: mvp * vertex.position.extend(1.),
                    color: paint(color(vertex.color), vertex.normal.truncate()),
                    uv: vertex.uv,
                }
            });
            self.triangle(triangle, None)?;
        }
        Ok(())
    }

    fn triangle(
        &mut self,
        triangle: [Vertex; 3],
        texture: Option<&RgbaImage>,
    ) -> Result<(), String> {
        self.spend(1)?;
        if triangle.iter().any(|vertex| !vertex.clip.is_finite()) {
            return Ok(());
        }
        // Clip the triangle against the camera's near plane before dividing by w.
        let distance = |v: Vertex| v.clip.z + v.clip.w;
        let mut clipped = [triangle[0]; 4];
        let mut used = 0;
        for index in 0..3 {
            let current = triangle[index];
            let previous = triangle[(index + 2) % 3];
            let a = distance(previous);
            let b = distance(current);
            if (a < 0.) != (b < 0.) {
                let t = a / (a - b);
                clipped[used] = Vertex {
                    clip: previous.clip.lerp(current.clip, t),
                    color: previous.color.lerp(current.color, t),
                    uv: previous.uv.lerp(current.uv, t),
                };
                used += 1;
            }
            if b >= 0. {
                clipped[used] = current;
                used += 1;
            }
        }
        for i in 2..used {
            self.rasterize([clipped[0], clipped[i - 1], clipped[i]], texture)?;
        }
        Ok(())
    }

    fn rasterize(
        &mut self,
        vertices: [Vertex; 3],
        texture: Option<&RgbaImage>,
    ) -> Result<(), String> {
        if vertices.iter().any(|vertex| vertex.clip.w <= 0.) {
            return Ok(());
        }
        let (width, height) = self.image.dimensions();
        let [a, b, c] = vertices.map(|vertex| {
            let point = vertex.clip.truncate() / vertex.clip.w;
            vec3(
                (point.x + 1.) * width as f32 * 0.5,
                (1. - point.y) * height as f32 * 0.5,
                point.z,
            )
        });
        let [lo, hi] = [a.min(b).min(c), a.max(b).max(c)];
        let x0 = lo.x.floor().max(0.) as u32;
        let y0 = lo.y.floor().max(0.) as u32;
        let x1 = (hi.x.ceil() as u32).min(width);
        let y1 = (hi.y.ceil() as u32).min(height);
        let edge =
            |p: Vec3, q: Vec3, r: Vec3| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
        let area = edge(a, b, c);
        if !area.is_finite() || area.abs() < 0.0001 || x0 >= x1 || y0 >= y1 {
            return Ok(());
        }
        self.spend(u64::from(x1 - x0) * u64::from(y1 - y0))?;
        let inv_w = Vec3::from_array(vertices.map(|vertex| vertex.clip.w.recip()));
        for y in y0..y1 {
            for x in x0..x1 {
                let p = vec3(x as f32 + 0.5, y as f32 + 0.5, 0.);
                let weight = vec3(edge(b, c, p), edge(c, a, p), edge(a, b, p)) / area;
                if weight.min_element() < 0. {
                    continue;
                }
                let depth = weight.dot(vec3(a.z, b.z, c.z));
                let index = (y * width + x) as usize;
                if depth < self.depth[index] && (-1.0001..=1.).contains(&depth) {
                    let weight = weight * inv_w;
                    let weight = weight / weight.element_sum();
                    let mut pixel = vertices[0].color * weight.x
                        + vertices[1].color * weight.y
                        + vertices[2].color * weight.z;
                    if let Some(texture) = texture {
                        let uv = vertices[0].uv * weight.x
                            + vertices[1].uv * weight.y
                            + vertices[2].uv * weight.z;
                        let u = (uv.x.clamp(0., 1.) * (texture.width() - 1) as f32).round() as u32;
                        let v = (uv.y.clamp(0., 1.) * (texture.height() - 1) as f32).round() as u32;
                        pixel *= color(texture.get_pixel(u, v).0);
                    }
                    if pixel.w <= 0. {
                        continue;
                    }
                    self.depth[index] = depth;
                    let out = self.image.get_pixel_mut(x, y);
                    let previous = Vec3::from_array(out.0.map(|channel| f32::from(channel) / 255.));
                    out.0 = previous
                        .lerp(pixel.truncate(), pixel.w)
                        .to_array()
                        .map(|component| (component * 255.).round() as u8);
                }
            }
        }
        Ok(())
    }

    fn spend(&mut self, work: u64) -> Result<(), String> {
        self.budget = self
            .budget
            .checked_sub(work)
            .ok_or("Preview is too complex. Reduce image size or visible geometry.")?;
        Ok(())
    }
}
