use macroquad::miniquad::{
    BlendFactor, BlendState, BlendValue, Comparison, Equation, PipelineParams, UniformDesc,
    UniformType,
};
use macroquad::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Lighting {
    pub direction: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
    pub ambient: f32,
    pub shadows: bool,
    pub shadow_extent: f32,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            direction: [0.4, 0.82, 0.41],
            color: [1., 0.96, 0.86],
            intensity: 2.6,
            ambient: 0.35,
            shadows: true,
            shadow_extent: 14.,
        }
    }
}

impl Lighting {
    pub fn validate(&self) -> bool {
        let direction = Vec3::from_array(self.direction);
        direction.is_finite()
            && (0.00001..=1e12).contains(&direction.length_squared())
            && valid_color(&self.color)
            && (0. ..=50.).contains(&self.intensity)
            && (0. ..=3.).contains(&self.ambient)
            && (1. ..=200.).contains(&self.shadow_extent)
    }

    pub(crate) fn sun(&self) -> Vec3 {
        Vec3::from_array(self.direction).normalize_or_zero()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PointLight {
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
}

impl Default for PointLight {
    fn default() -> Self {
        Self {
            color: [0.55, 0.75, 1.],
            intensity: 8.,
            range: 6.,
        }
    }
}

impl PointLight {
    pub fn validate(&self) -> bool {
        valid_color(&self.color)
            && (0. ..=200.).contains(&self.intensity)
            && (0.01..=200.).contains(&self.range)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alpha {
    Opaque,
    Mask,
    Blend,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Surface {
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: [f32; 3],
    pub unlit: bool,
    pub alpha: Alpha,
    pub alpha_cutoff: f32,
}

impl Default for Surface {
    fn default() -> Self {
        Self {
            base_color: [1.; 4],
            metallic: 0.,
            roughness: 0.7,
            emissive: [0.; 3],
            unlit: false,
            alpha: Alpha::Opaque,
            alpha_cutoff: 0.5,
        }
    }
}

impl Surface {
    pub fn validate(&self) -> bool {
        valid_color(&self.base_color)
            && self.emissive.iter().all(|v| (0. ..=50.).contains(v))
            && (0. ..=1.).contains(&self.metallic)
            && (0. ..=1.).contains(&self.roughness)
            && (0. ..=1.).contains(&self.alpha_cutoff)
    }

    fn cutoff(&self) -> f32 {
        match self.alpha {
            Alpha::Opaque => -2.,
            Alpha::Mask => self.alpha_cutoff,
            Alpha::Blend => -1.,
        }
    }
}

fn valid_color(color: &[f32]) -> bool {
    color.iter().all(|value| (0. ..=1.).contains(value))
}

pub(crate) struct Renderer {
    pub main: Material,
    pub depth: Material,
    shadow: RenderTarget,
    white: Texture2D,
}

impl Renderer {
    pub fn new() -> Result<Self, String> {
        let uniforms = [
            ("SunDirection", UniformType::Float3),
            ("SunColor", UniformType::Float3),
            ("Eye", UniformType::Float3),
            ("Ambient", UniformType::Float1),
            ("SunMatrix", UniformType::Mat4),
            ("NormalMatrix", UniformType::Mat4),
            ("UseShadow", UniformType::Float1),
            ("Tint", UniformType::Float4),
            ("Emission", UniformType::Float3),
            ("Params", UniformType::Float4),
            ("PointPosition0", UniformType::Float4),
            ("PointColor0", UniformType::Float3),
            ("PointPosition1", UniformType::Float4),
            ("PointColor1", UniformType::Float3),
            ("PointPosition2", UniformType::Float4),
            ("PointColor2", UniformType::Float3),
            ("PointPosition3", UniformType::Float4),
            ("PointColor3", UniformType::Float3),
        ]
        .into_iter()
        .map(|(n, t)| UniformDesc::new(n, t))
        .collect();
        let pipeline = PipelineParams {
            depth_test: Comparison::LessOrEqual,
            depth_write: true,
            ..Default::default()
        };
        let main = load_material(
            ShaderSource::Glsl {
                vertex: VERTEX,
                fragment: FRAGMENT,
            },
            MaterialParams {
                uniforms,
                textures: vec!["ShadowMap".into(), "MetalMap".into(), "EmissionMap".into()],
                pipeline_params: PipelineParams {
                    color_blend: Some(BlendState::new(
                        Equation::Add,
                        BlendFactor::Value(BlendValue::SourceAlpha),
                        BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                    )),
                    ..pipeline
                },
            },
        )
        .map_err(|e| e.to_string())?;
        let depth = load_material(
            ShaderSource::Glsl {
                vertex: DEPTH_VERTEX,
                fragment: DEPTH_FRAGMENT,
            },
            MaterialParams {
                uniforms: vec![
                    UniformDesc::new("AlphaCutoff", UniformType::Float1),
                    UniformDesc::new("TintAlpha", UniformType::Float1),
                ],
                pipeline_params: pipeline,
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
        let shadow = render_target_ex(
            1024,
            1024,
            RenderTargetParams {
                depth: true,
                sample_count: 1,
            },
        );
        shadow.texture.set_filter(FilterMode::Nearest);
        let white = Texture2D::from_rgba8(1, 1, &[255; 4]);
        main.set_texture("ShadowMap", shadow.texture.clone());
        Ok(Self {
            main,
            depth,
            shadow,
            white,
        })
    }

    pub fn shadow_camera(&self, target: Vec3, settings: &Lighting) -> Camera3D {
        let sun = settings.sun();
        let up = if sun.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y };
        let extent = settings.shadow_extent;
        Camera3D {
            position: target + sun * extent * 2.,
            target,
            up,
            projection: Projection::Orthographics,
            z_near: 0.1,
            z_far: extent * 4.,
            fovy: extent * 2.,
            aspect: Some(1.),
            render_target: Some(self.shadow.clone()),
            ..Default::default()
        }
    }

    pub fn lights(
        &self,
        camera: &Camera3D,
        shadow: &Camera3D,
        settings: &Lighting,
        points: &[(Vec3, &PointLight)],
    ) {
        self.main.set_uniform("SunDirection", settings.sun());
        self.main.set_uniform(
            "SunColor",
            Vec3::from_array(settings.color) * settings.intensity,
        );
        self.main.set_uniform("Eye", camera.position);
        self.main.set_uniform("Ambient", settings.ambient);
        self.main.set_uniform("SunMatrix", shadow.matrix());
        self.main
            .set_uniform("UseShadow", if settings.shadows { 1_f32 } else { 0. });
        for (i, (position_name, color_name)) in [
            ("PointPosition0", "PointColor0"),
            ("PointPosition1", "PointColor1"),
            ("PointPosition2", "PointColor2"),
            ("PointPosition3", "PointColor3"),
        ]
        .into_iter()
        .enumerate()
        {
            let (p, c) = points
                .get(i)
                .map_or((Vec4::ZERO, Vec3::ZERO), |(position, light)| {
                    (
                        position.extend(light.range),
                        Vec3::from_array(light.color) * light.intensity,
                    )
                });
            self.main.set_uniform(position_name, p);
            self.main.set_uniform(color_name, c);
        }
    }

    pub fn maps(&self, metal: Option<&Texture2D>, emissive: Option<&Texture2D>) {
        self.main
            .set_texture("MetalMap", metal.unwrap_or(&self.white).clone());
        self.main
            .set_texture("EmissionMap", emissive.unwrap_or(&self.white).clone());
    }

    pub fn surface(&self, surface: &Surface) {
        self.main
            .set_uniform("Tint", Vec4::from_array(surface.base_color));
        self.main
            .set_uniform("Emission", Vec3::from_array(surface.emissive));
        self.main.set_uniform(
            "Params",
            vec4(
                surface.metallic,
                surface.roughness,
                surface.cutoff(),
                if surface.unlit { 1. } else { 0. },
            ),
        );
    }

    pub fn caster(&self, surface: &Surface) {
        self.depth.set_uniform("AlphaCutoff", surface.cutoff());
        self.depth.set_uniform("TintAlpha", surface.base_color[3]);
    }
}

const VERTEX: &str = r#"
#version 100
precision highp float;
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;
uniform mat4 Projection, Model, NormalMatrix, SunMatrix;
varying mediump vec2 v_uv;
varying mediump vec4 v_color, v_shadow;
varying mediump vec3 v_normal, v_pos;
void main() {
    vec4 world = Model * vec4(position,1.0);
    gl_Position = Projection * world;
    v_uv = texcoord;
    v_color = color0 / 255.0;
    v_shadow = SunMatrix * world;
    v_normal = (NormalMatrix * vec4(normal.xyz,0.0)).xyz;
    v_pos = world.xyz;
}
"#;

const FRAGMENT: &str = r#"
#version 100
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
varying mediump vec2 v_uv;
varying mediump vec4 v_color, v_shadow;
varying mediump vec3 v_normal, v_pos;
uniform sampler2D Texture, ShadowMap, MetalMap, EmissionMap;
uniform vec3 SunDirection, SunColor, Eye, Emission;
uniform vec4 Tint, Params;
uniform float Ambient, UseShadow;
uniform vec4 PointPosition0, PointPosition1, PointPosition2, PointPosition3;
uniform vec3 PointColor0, PointColor1, PointColor2, PointColor3;
const float PI = 3.14159265;

float shadow_sample(vec2 uv, float depth) {
    vec2 stored = texture2D(ShadowMap, uv).rg;
    return step(depth, stored.x + stored.y / 255.0);
}
float shadow_factor(vec3 n) {
    if (UseShadow < 0.5) return 1.0;
    vec3 p = v_shadow.xyz / v_shadow.w * 0.5 + 0.5;
    if (p.x <= 0.0 || p.x >= 1.0 || p.y <= 0.0 || p.y >= 1.0 || p.z >= 1.0 || p.z <= 0.0) return 1.0;
    float depth = p.z - max(0.0024,0.0040 * (1.0 - max(dot(n,SunDirection),0.0)));
    float s = shadow_sample(p.xy,depth) + shadow_sample(p.xy+vec2(0.00098,0.0),depth)
        + shadow_sample(p.xy-vec2(0.00098,0.0),depth) + shadow_sample(p.xy+vec2(0.0,0.00098),depth)
        + shadow_sample(p.xy-vec2(0.0,0.00098),depth);
    return s * 0.2;
}
vec3 brdf(vec3 base, float metal, float rough, vec3 n, vec3 v, vec3 l, vec3 radiance) {
    float nl = max(dot(n,l),0.0);
    float nv = max(dot(n,v),0.001);
    if (nl <= 0.0) return vec3(0.0);
    vec3 h = normalize(v+l);
    float nh = max(dot(n,h),0.0);
    float vh = max(dot(v,h),0.0);
    vec3 f0 = mix(vec3(0.04),base,metal);
    vec3 f = f0 + (1.0-f0)*pow(1.0-vh,5.0);
    float a2 = rough*rough*rough*rough;
    float d = nh*nh*(a2-1.0)+1.0;
    float distribution = a2 / max(PI*d*d,0.0001);
    float k = (rough+1.0)*(rough+1.0)/8.0;
    float geometry = (nv/(nv*(1.0-k)+k))*(nl/(nl*(1.0-k)+k));
    vec3 specular = f*(distribution*geometry/max(4.0*nv*nl,0.001));
    vec3 diffuse = (1.0-f)*(1.0-metal)*base/PI;
    return (diffuse+specular)*radiance*nl;
}
vec3 point_light(vec4 point, vec3 radiance, vec3 base, float metal, float rough, vec3 n, vec3 v) {
    if (point.w <= 0.0) return vec3(0.0);
    vec3 delta = point.xyz-v_pos;
    float distance = max(length(delta),0.001);
    float falloff = max(1.0-pow(distance/point.w,4.0),0.0);
    return brdf(base,metal,rough,n,v,delta/distance,radiance*falloff*falloff/(1.0+distance*distance));
}
void main() {
    vec4 tex = texture2D(Texture,v_uv);
    vec4 paint = v_color * Tint;
    float alpha = tex.a*paint.a;
    if (Params.z >= 0.0 && alpha < Params.z) discard;
    alpha = Params.z >= 0.0 || Params.z < -1.5 ? 1.0 : alpha;
    vec3 base = pow(tex.rgb,vec3(2.2)) * paint.rgb;
    vec3 emitted = pow(texture2D(EmissionMap,v_uv).rgb,vec3(2.2))*Emission;
    if (Params.w > 0.5) {
        gl_FragColor = vec4(pow(clamp(base+emitted,0.0,1.0),vec3(1.0/2.2)),alpha);
        return;
    }
    vec3 n = normalize(v_normal);
    vec3 v = normalize(Eye-v_pos);
    vec3 channels = texture2D(MetalMap,v_uv).rgb;
    float metal = clamp(Params.x*channels.b,0.0,1.0);
    float rough = clamp(Params.y*channels.g,0.045,1.0);
    vec3 light = base*Ambient*(1.0-metal*0.7)+emitted;
    light += brdf(base,metal,rough,n,v,SunDirection,SunColor)*shadow_factor(n);
    light += point_light(PointPosition0,PointColor0,base,metal,rough,n,v);
    light += point_light(PointPosition1,PointColor1,base,metal,rough,n,v);
    light += point_light(PointPosition2,PointColor2,base,metal,rough,n,v);
    light += point_light(PointPosition3,PointColor3,base,metal,rough,n,v);
    gl_FragColor = vec4(pow(light/(vec3(1.0)+light),vec3(1.0/2.2)),alpha);
}
"#;

const DEPTH_VERTEX: &str = r#"
#version 100
precision highp float;
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
uniform mat4 Projection, Model;
varying mediump vec2 v_uv;
varying mediump float v_alpha;
void main() {
    gl_Position = Projection * Model * vec4(position,1.0);
    v_uv = texcoord;
    v_alpha = color0.a / 255.0;
}
"#;

const DEPTH_FRAGMENT: &str = r#"
#version 100
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
uniform sampler2D Texture;
uniform float AlphaCutoff, TintAlpha;
varying mediump vec2 v_uv;
varying mediump float v_alpha;
void main() {
    if (AlphaCutoff >= 0.0 && texture2D(Texture,v_uv).a*v_alpha*TintAlpha < AlphaCutoff) discard;
    float z = gl_FragCoord.z * 255.0;
    gl_FragColor = vec4(floor(z)/255.0,fract(z),0.0,1.0);
}
"#;
