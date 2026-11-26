use macroquad::miniquad::{
    BlendFactor, BlendState, BlendValue, Comparison, Equation, PipelineParams, UniformDesc,
    UniformType,
};
use macroquad::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Lighting {
    pub skybox: Skybox,
    pub direction: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
    pub ambient: f32,
    pub environment_strength: f32,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub color_shader: String,
    pub shadows: bool,
    pub shadow_extent: f32,
    pub shadow_profile: ShadowProfile,
    pub shadow_resolution: u16,
    pub mobile_shadow_extent: f32,
    pub mobile_shadow_resolution: u16,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            skybox: Skybox::Off,
            direction: [0.4, 0.82, 0.41],
            color: [1., 0.96, 0.86],
            intensity: 2.6,
            ambient: 0.35,
            environment_strength: 0.,
            color_shader: String::new(),
            shadows: true,
            shadow_extent: 14.,
            shadow_profile: ShadowProfile::Desktop,
            shadow_resolution: 1024,
            mobile_shadow_extent: 9.,
            mobile_shadow_resolution: 512,
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
            && (0. ..=3.).contains(&self.environment_strength)
            && (1. ..=200.).contains(&self.shadow_extent)
            && (1. ..=200.).contains(&self.mobile_shadow_extent)
            && SHADOW_RESOLUTIONS.contains(&self.shadow_resolution)
            && SHADOW_RESOLUTIONS.contains(&self.mobile_shadow_resolution)
    }

    pub fn shadow_settings(&self) -> (u16, f32) {
        match self.shadow_profile {
            ShadowProfile::Desktop => (self.shadow_resolution, self.shadow_extent),
            ShadowProfile::Mobile => (self.mobile_shadow_resolution, self.mobile_shadow_extent),
        }
    }

    pub(crate) fn sun(&self) -> Vec3 {
        Vec3::from_array(self.direction).normalize_or_zero()
    }
}

pub const SHADOW_RESOLUTIONS: [u16; 4] = [256, 512, 1024, 2048];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowProfile {
    #[default]
    Desktop,
    Mobile,
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
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub use_color_shader: bool,
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
            use_color_shader: false,
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

pub const PULSE_SHADER: &str = "return rgb * (0.65 + 0.35 * sin(time * 3.0 + uv.y * 6.28));";

pub fn validate_color_shader(source: &str) -> Result<(), String> {
    if source.len() > 16 * 1024 || source.contains('\0') {
        return Err("Color shaders support up to 16 KiB of GLSL without NUL bytes.".into());
    }
    Ok(())
}

pub(crate) struct Renderer {
    standard: Material,
    shader_source: String,
    pub main: Material,
    pub depth: Material,
    shadow: RenderTarget,
    shadow_resolution: u16,
    white: Texture2D,
}

impl Renderer {
    fn color_material(source: &str) -> Result<Material, String> {
        validate_color_shader(source)?;
        let body = if source.trim().is_empty() {
            "return rgb;"
        } else {
            source
        };
        let fragment = FRAGMENT.replace("/* COLOR_SHADER */", body);
        let uniforms = [
            ("SunDirection", UniformType::Float3),
            ("SunColor", UniformType::Float3),
            ("Eye", UniformType::Float3),
            ("Ambient", UniformType::Float1),
            ("EnvironmentStrength", UniformType::Float1),
            ("EnvironmentZenith", UniformType::Float3),
            ("EnvironmentHorizon", UniformType::Float3),
            ("EnvironmentGround", UniformType::Float3),
            ("ShadowTexel", UniformType::Float1),
            ("SoftShadows", UniformType::Float1),
            ("SunMatrix", UniformType::Mat4),
            ("NormalMatrix", UniformType::Mat4),
            ("NormalStrength", UniformType::Float1),
            ("ModelHandedness", UniformType::Float1),
            ("UseShadow", UniformType::Float1),
            ("UseColorShader", UniformType::Float1),
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
        load_material(
            ShaderSource::Glsl {
                vertex: VERTEX,
                fragment: &fragment,
            },
            MaterialParams {
                uniforms,
                textures: vec![
                    "ShadowMap".into(),
                    "MetalMap".into(),
                    "EmissionMap".into(),
                    "NormalMap".into(),
                ],
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
        .map_err(|e| e.to_string())
    }

    pub fn new(settings: &Lighting) -> Result<Self, String> {
        let main = Self::color_material("")?;
        let pipeline = PipelineParams {
            depth_test: Comparison::LessOrEqual,
            depth_write: true,
            ..Default::default()
        };
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
        let (shadow_resolution, _) = settings.shadow_settings();
        let shadow = shadow_target(shadow_resolution);
        let white = Texture2D::from_rgba8(1, 1, &[255; 4]);
        main.set_texture("ShadowMap", shadow.texture.clone());
        Ok(Self {
            standard: main.clone(),
            shader_source: String::new(),
            main,
            depth,
            shadow,
            shadow_resolution,
            white,
        })
    }

    pub fn apply_shader(&mut self, source: &str) -> Result<(), String> {
        let material = if source.trim().is_empty() {
            self.standard.clone()
        } else {
            Self::color_material(source)?
        };
        material.set_texture("ShadowMap", self.shadow.texture.clone());
        self.main = material;
        self.shader_source = source.to_owned();
        Ok(())
    }

    pub fn sync_shader(&mut self, source: &str) {
        if self.shader_source != source {
            if let Err(error) = self.apply_shader(source) {
                macroquad::logging::error!("Color shader: {}", error);
                self.main = self.standard.clone();
                self.shader_source = source.to_owned();
            }
        }
    }

    pub fn sync_shadow_profile(&mut self, settings: &Lighting) {
        let (resolution, _) = settings.shadow_settings();
        if self.shadow_resolution == resolution {
            return;
        }
        self.shadow = shadow_target(resolution);
        self.shadow_resolution = resolution;
        self.standard
            .set_texture("ShadowMap", self.shadow.texture.clone());
        self.main
            .set_texture("ShadowMap", self.shadow.texture.clone());
    }

    pub fn shadow_camera(&self, target: Vec3, settings: &Lighting) -> Camera3D {
        let sun = settings.sun();
        let up = if sun.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y };
        let (_, extent) = settings.shadow_settings();
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
        let environment = settings.skybox.palette();
        let strength = if environment.is_some() {
            settings.environment_strength
        } else {
            0.
        };
        self.main.set_uniform("EnvironmentStrength", strength);
        if let Some((top, edge, low)) = environment.filter(|_| strength > 0.) {
            let linear =
                |color: Vec3| Vec3::new(color.x.powf(2.2), color.y.powf(2.2), color.z.powf(2.2));
            self.main.set_uniform("EnvironmentZenith", linear(top));
            self.main.set_uniform("EnvironmentHorizon", linear(edge));
            self.main.set_uniform("EnvironmentGround", linear(low));
        }
        self.main
            .set_uniform("ShadowTexel", 1. / f32::from(self.shadow_resolution));
        self.main.set_uniform(
            "SoftShadows",
            f32::from(settings.shadow_profile == ShadowProfile::Desktop),
        );
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

    pub fn maps(&self, maps: [Option<&Texture2D>; 3]) {
        for (name, map) in ["MetalMap", "EmissionMap", "NormalMap"]
            .into_iter()
            .zip(maps)
        {
            self.main
                .set_texture(name, map.unwrap_or(&self.white).clone());
        }
    }

    pub fn surface(&self, surface: &Surface) {
        self.main
            .set_uniform("Tint", Vec4::from_array(surface.base_color));
        self.main
            .set_uniform("Emission", Vec3::from_array(surface.emissive));
        self.main
            .set_uniform("UseColorShader", f32::from(surface.use_color_shader));
        self.main.set_uniform(
            "Params",
            vec4(
                surface.metallic,
                surface.roughness,
                surface.cutoff(),
                f32::from(surface.unlit),
            ),
        );
    }

    pub fn caster(&self, surface: &Surface) {
        self.depth.set_uniform("AlphaCutoff", surface.cutoff());
        self.depth.set_uniform("TintAlpha", surface.base_color[3]);
    }
}

fn shadow_target(resolution: u16) -> RenderTarget {
    let size = u32::from(resolution);
    let target = render_target_ex(
        size,
        size,
        RenderTargetParams {
            depth: true,
            sample_count: 1,
        },
    );
    target.texture.set_filter(FilterMode::Nearest);
    target
}

const VERTEX: &str = r#"
#version 100
precision highp float;
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;
uniform mat4 Projection, Model, NormalMatrix, SunMatrix;
uniform float ModelHandedness;
varying mediump vec2 v_uv;
varying mediump vec4 v_color, v_shadow;
varying mediump vec3 v_normal, v_pos;
varying mediump vec4 v_tangent;
void main() {
    vec4 world = Model * vec4(position,1.0);
    gl_Position = Projection * world;
    v_uv = texcoord;
    v_color = color0 / 255.0;
    v_shadow = SunMatrix * world;
    v_normal = (NormalMatrix * vec4(normal.xyz,0.0)).xyz;
    // Macroquad exposes four normal components. The last carries the
    // tangent angle and its handedness, decoded before interpolation.
    v_tangent = vec4(0.0);
    if (abs(normal.w) > 0.5) {
        vec3 n = normalize(normal.xyz);
        vec3 basis = normalize(cross(abs(n.y) < 0.9 ? vec3(0.,1.,0.) : vec3(1.,0.,0.), n));
        float angle = (abs(normal.w) - 1.5) * 6.28318530718;
        vec3 t = cos(angle)*basis + sin(angle)*cross(n,basis);
        v_tangent = vec4(normalize((Model * vec4(t,0.)).xyz), sign(normal.w)*ModelHandedness);
    }
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
varying mediump vec4 v_tangent;
uniform sampler2D Texture, ShadowMap, MetalMap, EmissionMap, NormalMap;
uniform vec3 SunDirection, SunColor, Eye, Emission;
uniform vec4 Tint, Params;
uniform float Ambient, UseShadow, UseColorShader, NormalStrength;
uniform float EnvironmentStrength, ShadowTexel, SoftShadows;
uniform vec3 EnvironmentZenith, EnvironmentHorizon, EnvironmentGround;
uniform vec4 PointPosition0, PointPosition1, PointPosition2, PointPosition3;
uniform vec3 PointColor0, PointColor1, PointColor2, PointColor3;
uniform vec4 _Time;
vec3 forge_color(vec3 rgb, vec2 uv, float time) {
    /* COLOR_SHADER */
}
vec3 finish_color(vec3 rgb) {
    if (UseColorShader < 0.5) return rgb;
    return clamp(forge_color(rgb, v_uv, _Time.x), 0.0, 1.0);
}
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
    float s = shadow_sample(p.xy,depth);
    if (SoftShadows < 0.5) return s;
    vec2 texel = vec2(ShadowTexel,0.0);
    s += shadow_sample(p.xy+texel.xy,depth) + shadow_sample(p.xy-texel.xy,depth)
       + shadow_sample(p.xy+texel.yx,depth) + shadow_sample(p.xy-texel.yx,depth);
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
vec3 sky_radiance(float height) {
    vec3 upper = mix(EnvironmentHorizon, EnvironmentZenith, smoothstep(0.0,0.78,height));
    return mix(upper, EnvironmentGround, smoothstep(0.0,0.65,-height));
}
vec3 environment_light(vec3 base, float metal, float rough, vec3 n, vec3 v) {
    if (EnvironmentStrength <= 0.0) return vec3(0.0);
    vec3 average = EnvironmentZenith*0.35 + EnvironmentHorizon*0.4 + EnvironmentGround*0.25;
    vec3 diffuse = mix(average, sky_radiance(n.y),0.45) * base * (1.0-metal);
    vec3 reflection = reflect(-v,n);
    vec3 reflected = mix(sky_radiance(reflection.y),average,rough*rough);
    float grazing = pow(1.0-clamp(dot(n,v),0.0,1.0),5.0);
    vec3 f0 = mix(vec3(0.04),base,metal);
    vec3 specular = reflected * (f0 + (1.0-f0)*grazing);
    return EnvironmentStrength * (diffuse + specular);
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
        gl_FragColor = vec4(finish_color(pow(clamp(base+emitted,0.0,1.0),vec3(1.0/2.2))),alpha);
        return;
    }
    vec3 geometric = normalize(v_normal);
    vec3 n = geometric;
    if (NormalStrength != 0.0 && abs(v_tangent.w) > 0.5) {
        vec3 projected = v_tangent.xyz - n * dot(n,v_tangent.xyz);
        if (dot(projected,projected) > 0.00001) {
            vec3 t = normalize(projected);
            vec3 b = cross(n,t) * sign(v_tangent.w);
            vec3 detail = texture2D(NormalMap,v_uv).rgb * 2.0 - 1.0;
            vec3 world = t*(detail.x*NormalStrength) + b*(detail.y*NormalStrength) + n*detail.z;
            if (dot(world,world) > 0.00001) n = normalize(world);
        }
    }
    vec3 v = normalize(Eye-v_pos);
    vec3 channels = texture2D(MetalMap,v_uv).rgb;
    float metal = clamp(Params.x*channels.b,0.0,1.0);
    float rough = clamp(Params.y*channels.g,0.045,1.0);
    vec3 light = base*Ambient*(1.0-metal*0.7)+emitted;
    light += environment_light(base,metal,rough,n,v);
    light += brdf(base,metal,rough,n,v,SunDirection,SunColor)*shadow_factor(geometric);
    light += point_light(PointPosition0,PointColor0,base,metal,rough,n,v);
    light += point_light(PointPosition1,PointColor1,base,metal,rough,n,v);
    light += point_light(PointPosition2,PointColor2,base,metal,rough,n,v);
    light += point_light(PointPosition3,PointColor3,base,metal,rough,n,v);
    gl_FragColor = vec4(finish_color(pow(light/(vec3(1.0)+light),vec3(1.0/2.2))),alpha);
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

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Skybox {
    #[default]
    Off,
    Day,
    Sunset,
    Night,
}

pub(crate) fn sky_material() -> Result<Material, String> {
    load_material(
        ShaderSource::Glsl {
            vertex: r#"#version 100
precision highp float;
attribute vec3 position;
uniform mat4 Projection;
uniform vec3 Eye;
varying mediump vec3 ray;
void main(){
    ray=position;
    vec4 clip=Projection*vec4(Eye+position*16.0,1.0);
    gl_Position=clip.xyww;
}"#,
            fragment: r#"#version 100
precision mediump float;
varying mediump vec3 ray;
uniform vec3 Zenith,Horizon,Low;
uniform float Night;
void main(){
    vec3 d=normalize(ray);
    float h=d.y;
    vec3 col=mix(Horizon,Zenith,smoothstep(-0.03,0.48,h));
    vec3 sun=normalize(vec3(-0.35,0.09,-1.0));
    float dist=length(d-sun);
    col+=vec3(1.0,0.68,0.35)*(0.16*exp(-4.0*dist)+0.70*(1.0-smoothstep(0.055,0.061,dist)));
    float ridge=-0.105+0.040*sin(d.x*17.0+d.z*8.0)+0.014*cos(d.z*38.0-d.x*23.0);
    col=mix(mix(Low,Horizon,0.34),col,smoothstep(ridge-0.004,ridge+0.004,h));
    float nearRidge=-0.18+0.033*cos(d.x*12.0-d.z*13.0);
    col=mix(Low,col,smoothstep(nearRidge-0.005,nearRidge+0.005,h));
    vec3 grid=d*90.0;
    vec3 cell=floor(grid);
    float seed=fract(sin(dot(mod(cell,16.0),vec3(2.1,4.7,7.1)))*137.3);
    float star=(1.0-smoothstep(0.06,0.22,length(fract(grid)-0.5)))*step(0.88,seed);
    col+=vec3(star*Night*smoothstep(0.0,0.22,h));
    gl_FragColor=vec4(col,1.0);
}"#,
        },
        MaterialParams {
            uniforms: vec![
                UniformDesc::new("Eye", UniformType::Float3),
                UniformDesc::new("Zenith", UniformType::Float3),
                UniformDesc::new("Horizon", UniformType::Float3),
                UniformDesc::new("Low", UniformType::Float3),
                UniformDesc::new("Night", UniformType::Float1),
            ],
            pipeline_params: PipelineParams {
                depth_write: false,
                depth_test: Comparison::Always,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())
}

impl Skybox {
    pub(crate) fn palette(self) -> Option<(Vec3, Vec3, Vec3)> {
        Some(match self {
            Self::Off => return None,
            Self::Day => (
                vec3(0.08, 0.38, 0.72),
                vec3(0.68, 0.88, 0.94),
                vec3(0.19, 0.40, 0.49),
            ),
            Self::Sunset => (
                vec3(0.13, 0.16, 0.43),
                vec3(1.0, 0.63, 0.38),
                vec3(0.25, 0.17, 0.32),
            ),
            Self::Night => (
                vec3(0.018, 0.035, 0.14),
                vec3(0.23, 0.32, 0.54),
                vec3(0.07, 0.09, 0.19),
            ),
        })
    }
    pub(crate) fn draw(self, mat: &Material, cube: &Mesh, eye: Vec3) {
        let Some((top, edge, low)) = self.palette() else {
            return;
        };
        mat.set_uniform("Eye", eye);
        mat.set_uniform("Zenith", top);
        mat.set_uniform("Horizon", edge);
        mat.set_uniform("Low", low);
        mat.set_uniform("Night", if self == Self::Night { 1f32 } else { 0. });
        gl_use_material(mat);
        draw_mesh(cube);
        gl_use_default_material();
    }
}
