//! The vault door, drawn live by a shader.
//!
//! A port of the landing page's fragment shader (`site/door.js`) to WGSL, run through Rune's
//! compositor hook. Views place an effect slot with [`door`]; the compositor paints the steel
//! there, at whatever size and place the slot has, in the window and in a headless render alike.
//!
//! The web shader works in canvas pixels with the origin at the bottom left. Here the pixel
//! comes from `@builtin(position)` and the pass's own origin, so it is in window device pixels
//! with the origin at the top left; no flip is needed. Its colours are sRGB; the output is
//! premultiplied, encoded for whichever target the pass draws into (`EffectSite::srgb_blend`).

use std::collections::HashMap;

use crate::state::State;
use rune::{
    El,
    compose::{Compositor, Frame},
    el,
    render::EffectSite,
};

/// The effect slot that draws the door.
pub const DOOR: u32 = 1;

const UNIFORM_SIZE: usize = 64;
const MAX_SLOTS: usize = 8;

/// The door's resting light: the page's default, up and to the left.
const LIGHT: [f32; 2] = [-0.55, -0.65];

/// A slot of `size` points that the compositor paints the door into. It draws nothing itself:
/// the effect keeps the slot's place, rect, clip and opacity.
pub fn door(size: f32) -> El {
    el().size(size, size).effect(DOOR).a11y_hidden()
}

/// The compositor: Rune draws the frame, and this paints the door at each `DOOR` slot.
#[derive(Default)]
pub struct Gfx {
    gpu: Option<Gpu>,
    /// The frame that first found the compositor, whose view still shows the picture: draw one
    /// more so the door appears without waiting for the owner to touch something.
    wake: bool,
}

struct Gpu {
    key: rune::compose::DeviceKey,
    layout: wgpu::PipelineLayout,
    module: wgpu::ShaderModule,
    pipes: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    uniforms: wgpu::Buffer,
    bind: wgpu::BindGroup,
    /// Bytes between one slot's uniforms and the next: the adapter's offset alignment, which is
    /// larger than the 64 bytes themselves.
    stride: u32,
}

impl Gpu {
    fn new(device: &wgpu::Device, key: rune::compose::DeviceKey) -> Gpu {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("door"),
            source: wgpu::ShaderSource::Wgsl(DOOR_WGSL.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("door"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_SIZE as u64),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("door"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let stride = (UNIFORM_SIZE as u32).max(device.limits().min_uniform_buffer_offset_alignment);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("door uniforms"),
            size: (stride as usize * MAX_SLOTS) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("door"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniforms,
                    offset: 0,
                    size: wgpu::BufferSize::new(UNIFORM_SIZE as u64),
                }),
            }],
        });
        Gpu {
            key,
            layout,
            module,
            pipes: HashMap::new(),
            uniforms,
            bind,
            stride,
        }
    }

    fn pipeline(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> &wgpu::RenderPipeline {
        self.pipes.entry(format).or_insert_with(|| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("door"),
                layout: Some(&self.layout),
                vertex: wgpu::VertexState {
                    module: &self.module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }
}

/// One slot's uniforms, laid out as `U` in the shader.
struct Slot {
    center: [f32; 2],
    radius: f32,
    split: f32,
    light: [f32; 2],
    spin: f32,
    gain: f32,
    origin: [f32; 2],
    encode: f32,
}

impl Slot {
    fn bytes(&self) -> [u8; UNIFORM_SIZE] {
        let words = [
            self.center[0],
            self.center[1],
            self.radius,
            self.split,
            self.light[0],
            self.light[1],
            self.spin,
            self.gain,
            self.origin[0],
            self.origin[1],
            self.encode,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        let mut bytes = [0u8; UNIFORM_SIZE];
        for (to, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(words) {
            *to = word.to_le_bytes();
        }
        bytes
    }
}

/// The slot's rect and radius in window device pixels. The door fills the mark as the PNG does:
/// its rim sits 1/512 of the box in from the edge.
fn geometry(site: &EffectSite<'_>) -> Slot {
    let [x, y, w, h] = site.instance.rect;
    let side = w.min(h);
    Slot {
        center: [x + w / 2.0, y + h / 2.0],
        radius: side * (0.5 - 1.0 / 512.0),
        split: 0.0,
        light: LIGHT,
        spin: 0.0,
        gain: 1.0,
        origin: site.origin,
        encode: if site.srgb_blend { 1.0 } else { 0.0 },
    }
}

impl Compositor<State> for Gfx {
    fn animating(&self) -> bool {
        self.wake
    }

    fn render(&mut self, f: &mut Frame<'_, State>) {
        let (device, queue) = (f.device, f.queue);
        let key = f.device_key();
        if self.gpu.as_ref().is_none_or(|g| g.key != key) {
            self.gpu = Some(Gpu::new(device, key));
        }
        let gpu = self.gpu.as_mut().expect("just made");
        let clear = f.rt.clear_color();
        // The view was built without knowing a compositor is here. The next frame draws the
        // shader door, and `poll` commits it; this keeps the display link on until that frame.
        self.wake = !f.rt.state.shaders;
        f.rt.state.shaders = true;
        let mut slot = 0usize;
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, f.size);
        f.ui.draw_with(f.encoder, f.target, clear, &mut |pass, site| {
            if site.mark != DOOR {
                return;
            }
            let at = geometry(site);
            if slot < MAX_SLOTS {
                let offset = slot as u64 * gpu.stride as u64;
                queue.write_buffer(&gpu.uniforms, offset, &at.bytes());
                slot += 1;
                pass.set_pipeline(gpu.pipeline(device, site.format));
                pass.set_bind_group(0, &gpu.bind, &[(slot as u32 - 1) * gpu.stride]);
                pass.draw(0..3, 0..1);
            }
        });
    }
}

const DOOR_WGSL: &str = r#"
struct U {
    center: vec2f,
    radius: f32,
    split: f32,
    light: vec2f,
    spin: f32,
    gain: f32,
    origin: vec2f,
    encode: f32,
    pad: f32,
};

@group(0) @binding(0) var<uniform> u: U;

const RINGS: i32 = 5;
const EDGE = array<f32, 6>(0.0, 0.1875, 0.3906, 0.6094, 0.7969, 0.9922);
// Lit (left) and shaded (right) halves of assets/icons/icon.svg, inner ring first.
const LIT = array<vec3f, 5>(
    vec3f(0.824, 0.847, 0.863), vec3f(0.647, 0.698, 0.729), vec3f(0.294, 0.392, 0.459),
    vec3f(0.235, 0.322, 0.376), vec3f(0.149, 0.200, 0.231));
const DIM = array<vec3f, 5>(
    vec3f(0.780, 0.816, 0.831), vec3f(0.565, 0.631, 0.667), vec3f(0.125, 0.255, 0.329),
    vec3f(0.110, 0.208, 0.278), vec3f(0.067, 0.129, 0.173));
const DIRECTION = array<f32, 5>(0.0, -1.0, 1.0, -0.6, 0.25);

fn to_lin(c: vec3f) -> vec3f {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3f(0.0)) + 0.055) / 1.055, vec3f(2.4));
    return select(hi, lo, c <= vec3f(0.04045));
}

fn hash(x: f32) -> f32 { return fract(sin(x * 127.1) * 43758.5453); }
fn hash2(p: vec2f) -> f32 { return fract(sin(dot(p, vec2f(12.9898, 78.233))) * 43758.5453); }
fn noise(x: f32) -> f32 {
    let i = floor(x);
    let f = fract(x);
    return mix(hash(i), hash(i + 1.0), f * f * (3.0 - 2.0 * f));
}

fn glsl_mod(a: f32, b: f32) -> f32 { return a - b * floor(a / b); }

// One half of the door, with premultiplied alpha.
fn half_door(world: vec2f, side: f32) -> vec4f {
    let p = world - vec2f(side * u.split, 0.0);
    let px = 1.0 / u.radius;
    let inside = smoothstep(-px, px, p.x * side - px * 0.75);
    let r = length(p);
    let outer = 1.0 - smoothstep(EDGE[RINGS] - px, EDGE[RINGS] + px, r);
    let alpha = inside * outer;
    if (alpha <= 0.0) { return vec4f(0.0); }

    var ring = 0;
    for (var i = 0; i < RINGS; i++) {
        if (r >= EDGE[i + 1]) { ring = i + 1; }
    }
    ring = min(ring, RINGS - 1);
    let a = EDGE[ring];
    let b = EDGE[ring + 1];
    let seed = f32(ring) * 17.3;
    // At the very centre there's no angle to speak of, and atan2 may return anything.
    let angle = select(0.0, atan2(p.y, p.x), r > 0.002);
    let turned = angle + u.spin * DIRECTION[ring];

    let radial = select(vec2f(1.0, 0.0), p / r, r > 0.0001);
    let tangent = vec2f(-radial.y, radial.x);

    // Lathe marks: finer than a pixel is averaged away so zooming in never shimmers.
    let detail = clamp(u.radius * 0.9, 120.0, 1400.0);
    let lines = noise(r * detail + seed) * 0.6 + noise(r * detail * 0.31 + seed) * 0.4;
    let blotch = noise(turned * 3.0 + seed) * 0.5 + noise(turned * 11.0 + seed * 2.0) * 0.5;

    // The rim of each ring catches light like a chamfer.
    let width = 0.016;
    let in_dist = r - a;
    let out_dist = b - r;
    let chamfer = smoothstep(width, 0.0, out_dist) - smoothstep(width, 0.0, in_dist);
    let normal = normalize(vec3f(radial * chamfer * 0.9, 1.0));

    let to_light = normalize(vec3f(u.light - p, 1.3));
    let halfway = normalize(to_light + vec3f(0.0, 0.0, 1.0));

    // Anisotropic highlight: brushed circles flare along the radius, in opposing fans.
    let brushed = normalize(tangent + radial * (lines - 0.5) * 0.09);
    let along = dot(brushed, halfway.xy);
    let fan = exp(-along * along * 150.0) * 0.9 + exp(-along * along * 14.0) * 0.28;
    let diffuse = max(dot(normal, to_light), 0.0);

    var base = select(DIM[ring], LIT[ring], side < 0.0);
    base *= 0.92 + (lines - 0.5) * 0.22 + (blotch - 0.5) * 0.06;

    let strength = mix(0.72, 0.5, f32(ring) / 4.0) * u.gain;
    let steel = vec3f(0.84, 0.91, 0.97);
    var c = base * (0.5 + 0.55 * diffuse)
        + steel * fan * strength * diffuse * select(0.72, 1.0, side < 0.0);
    c += steel * max(chamfer * dot(radial, normalize(u.light - p)), 0.0) * 0.5 * u.gain;

    // The groove between rings.
    let groove = smoothstep(0.0, 0.007, min(in_dist, out_dist));
    c *= mix(0.16, 1.0, groove);

    // The dial: graduations cut into the fourth ring, turning with it.
    if (ring == 3) {
        let count = 72.0;
        let cell = turned / 6.2831853 * count;
        let tick = abs(fract(cell) - 0.5) * 2.0;
        let major = glsl_mod(floor(cell), 6.0) < 0.5;
        let reach = select(0.026, 0.05, major);
        let cut = smoothstep(0.9, 0.96, tick) * step(b - 0.018 - reach, r) * step(r, b - 0.018);
        c = mix(c, c * 0.34, cut);
    }

    // Bolts around the third ring, each a small lit dome.
    var d = 0.0;
    if (ring == 2) {
        let count = 12.0;
        let step_ = 6.2831853 / count;
        let cell_angle = glsl_mod(turned + step_ * 0.5, step_) - step_ * 0.5;
        let mid = (a + b) * 0.5;
        let local = vec2f(r * cos(cell_angle) - mid, r * sin(cell_angle));
        d = length(local) / 0.024;
        if (d < 1.0) {
            let dome = sqrt(1.0 - d * d);
            let bolt_normal = normalize(vec3f(local / 0.024, dome + 0.2));
            let bolt_light = pow(max(dot(bolt_normal, halfway), 0.0), 26.0);
            c = base * 0.5 + base * 0.7 * max(dot(bolt_normal, to_light), 0.0) + steel * bolt_light * 0.9;
        }
        c *= mix(0.55, 1.0, smoothstep(0.92, 1.12, d));
    }

    // The cut faces of the two halves.
    let face_dist = abs(p.x);
    c += steel * smoothstep(0.012, 0.0, face_dist) * 0.14 * smoothstep(0.0, 0.02, u.split);
    c *= mix(0.25, 1.0, smoothstep(0.0, 0.0035, face_dist));

    // A last guard: one bad pixel in the hub is the first thing anyone would see.
    if (!(c.r >= 0.0 && c.g >= 0.0 && c.b >= 0.0)) { c = base; }
    return vec4f(clamp(c, vec3f(0.0), vec3f(1.0)) * alpha, alpha);
}

@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let p = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    // The pass's own top-left may sit inside the window (an isolated layer): shift to window
    // device pixels, with the origin Rune draws from, the top left.
    let frag = pos.xy + u.origin;
    let world = (frag - u.center) / u.radius;
    let left = half_door(world, -1.0);
    let right = half_door(world, 1.0);
    let door = left + right * (1.0 - left.a);

    // No shadow: the page behind the web canvas is the wall its shadow falls on, while the app's
    // door sits on its own page, and the picture it replaces had none.

    let a = door.a;
    var rgb = door.rgb;
    rgb += (hash2(frag) - 0.5) / 180.0 * door.a;

    // The shader works in sRGB: its values are already encoded. A pass whose frame blends in
    // linear light wants them decoded (an sRGB target encodes them on write); a frame that blends
    // in gamma space (`Frame::srgb_blend`) stores them as they are. Either way, premultiplied.
    let straight = select(vec3f(0.0), rgb / a, a > 0.000001);
    let encoded = select(to_lin(straight), straight, u.encode > 0.5);
    return vec4f(encoded * a, a);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_door_fills_its_box_as_the_picture_does() {
        // The icon was rendered with a radius of 510 in a 1024-pixel canvas.
        let centre_radius = 0.5f32;
        assert!((centre_radius - 1.0 / 512.0 - 510.0f32 / 1024.0).abs() < 1e-6);
    }

    #[test]
    fn a_slot_packs_its_numbers_as_the_shader_reads_them() {
        let slot = Slot {
            center: [1.0, 2.0],
            radius: 3.0,
            split: 4.0,
            light: [5.0, 6.0],
            spin: 7.0,
            gain: 8.0,
            origin: [9.0, 10.0],
            encode: 11.0,
        };
        let bytes = slot.bytes();
        let word = |i: usize| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        let numbers: Vec<f32> = (0..11).map(word).collect();
        assert_eq!(
            numbers,
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0]
        );
    }
}
