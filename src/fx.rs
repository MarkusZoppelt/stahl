//! The vault door, drawn live by a shader.
//!
//! A port of the landing page's fragment shader (`site/door.js`) to WGSL, run through Rune's
//! compositor hook. Views place an effect slot with [`door`], [`mark`], [`pull`] or [`opening`];
//! the compositor paints the steel there, at whatever size and place the slot has, in the window
//! and in a headless render alike.
//!
//! The web shader works in canvas pixels with the origin at the bottom left. Here the pixel
//! comes from `@builtin(position)` and the pass's own origin, so it is in window device pixels
//! with the origin at the top left; no flip is needed. Its colours are sRGB; the output is
//! premultiplied, encoded for whichever target the pass draws into (`EffectSite::srgb_blend`).

use std::{
    collections::{HashMap, HashSet},
    f32::consts::TAU,
};

use crate::state::State;
use rune::{
    El, Transition,
    compose::{Compositor, Frame},
    el,
    render::EffectSite,
};

/// The large door: the welcome screen's, and the one that opens a portfolio.
pub const DOOR: u32 = 1;
/// The small mark: the sidebar's, the phone's top bar.
pub const MARK: u32 = 2;
/// The door behind a page pulled past its top on a phone.
pub const PULL: u32 = 3;
/// The full-window slot that plays the opening when a portfolio first shows.
pub const OPEN: u32 = 4;

const UNIFORM_SIZE: usize = 64;
const MAX_SLOTS: usize = 8;

/// How long the opening takes, as on the landing page.
const OPENING: f32 = 1.8;
/// How far the tumblers turn while the door pushes in.
const OPENING_SPIN: f32 = 2.4;

/// The door's resting light: the page's default, up and to the left.
const LIGHT: [f32; 2] = [-0.55, -0.65];

/// How heavy the tumblers are: a turn settles in about a second, with a small give.
const SPRING: f32 = 70.0;
const DAMPING: f32 = 10.0;

/// The light follows a pointer within two radii of the door, as the page's does.
const LIGHT_REACH: f32 = 2.0;
/// The light eases 6% a frame at 60, as on the page.
const LIGHT_EASE: f32 = 0.06;

/// A slot of `size` points that the compositor paints the door into. It draws nothing itself:
/// the effect keeps the slot's place, rect, clip and opacity. `turns` is how far the tumblers
/// have been turned; the compositor eases the real spin towards it.
fn slot(size: f32, mark: u32, turns: f32) -> El {
    el().size(size, size)
        .effect(mark)
        .effect_data([turns, 0.0, 0.0, 0.0])
        .a11y_hidden()
}

/// The large door, at `size`.
pub fn door(size: f32, turns: u64) -> El {
    slot(size, DOOR, turns as f32)
}

/// The small mark, at `size`.
pub fn mark(size: f32, turns: u64) -> El {
    slot(size, MARK, turns as f32)
}

/// The door behind a pull, at `size`. It never turns.
pub fn pull(size: f32) -> El {
    slot(size, PULL, 0.0)
}

/// The full-window slot that plays the opening, over the page. Its payload is when it started
/// (on the runtime's clock) and the door's resting centre and radius in points; a centre x below
/// zero means the window's middle.
pub fn opening(started: f64, center: [f32; 2], radius: f32) -> El {
    el().absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .effect(OPEN)
        .effect_data([started as f32, center[0], center[1], radius])
        .a11y_hidden()
        .transition(Transition::none())
}

/// What every door needs from the frame it is drawn in.
#[derive(Clone, Copy)]
struct FrameFacts {
    /// The runtime's clock, so easing is deterministic headless.
    t: f64,
    reduced: bool,
    pointer: Option<(f32, f32)>,
}

/// One door's live state: where its light is, where it is heading, and how far its tumblers
/// have turned. The compositor keeps this, so a moving light never rebuilds the view.
struct Door {
    light: [f32; 2],
    target: [f32; 2],
    /// Turns, eased; the shader wants radians.
    spin: f32,
    spin_target: f32,
    speed: f32,
    /// The last turn count the view asked for.
    turns: f32,
    /// The clock when this door last stepped, so a frame that draws it twice advances it once.
    t: f64,
}

impl Door {
    /// A door seen for the first time starts its tumblers at rest and turns them to `turns`, as
    /// a freshly built dial did, so a refresh shown by a new mark still turns.
    fn new(turns: f32, t: f64) -> Door {
        Door {
            light: LIGHT,
            target: LIGHT,
            spin: 0.0,
            spin_target: turns,
            speed: 0.0,
            turns,
            t,
        }
    }

    fn step(&mut self, frame: FrameFacts, scale: f32, center: [f32; 2], radius: f32, turns: f32) {
        if frame.t <= self.t {
            return;
        }
        let dt = ((frame.t - self.t) as f32).min(0.05);
        self.t = frame.t;
        self.target = match frame.pointer {
            Some((x, y)) => {
                let reach = radius.max(1.0);
                [
                    ((x * scale - center[0]) / reach).clamp(-LIGHT_REACH, LIGHT_REACH),
                    ((y * scale - center[1]) / reach).clamp(-LIGHT_REACH, LIGHT_REACH),
                ]
            }
            None => LIGHT,
        };
        if turns != self.turns {
            self.turns = turns;
            self.spin_target = turns;
        }
        if frame.reduced {
            self.light = self.target;
            self.spin = self.spin_target;
            self.speed = 0.0;
            return;
        }
        // 6% a frame at 60, whatever the frame rate.
        let k = 1.0 - (1.0 - LIGHT_EASE).powf(dt * 60.0);
        self.light[0] += (self.target[0] - self.light[0]) * k;
        self.light[1] += (self.target[1] - self.light[1]) * k;
        // A heavy, slightly sprung settle, in fixed steps so a long frame can't blow it up.
        let mut left = dt;
        while left > 0.0 {
            let h = left.min(1.0 / 240.0);
            let pull = -SPRING * (self.spin - self.spin_target) - DAMPING * self.speed;
            self.speed += pull * h;
            self.spin += self.speed * h;
            left -= h;
        }
        if (self.spin - self.spin_target).abs() < 1e-4 && self.speed.abs() < 1e-2 {
            self.spin = self.spin_target;
            self.speed = 0.0;
        }
    }

    fn moving(&self) -> bool {
        (self.light[0] - self.target[0]).abs() > 5e-4
            || (self.light[1] - self.target[1]).abs() > 5e-4
            || (self.spin - self.spin_target).abs() > 5e-4
            || self.speed.abs() > 1e-2
    }
}

/// The compositor: Rune draws the frame, and this paints the door at each door slot.
#[derive(Default)]
pub struct Gfx {
    gpu: Option<Gpu>,
    doors: HashMap<u32, Door>,
    /// The frame that first found the compositor, whose view still shows the picture: draw one
    /// more so the door appears without waiting for the owner to touch something.
    wake: bool,
    /// The opening was drawn this frame and has not finished: keep the display link running.
    opening: bool,
    /// When the opening started on the animation clock, so its progress and the state's timer
    /// share one clock even after the host slept.
    opening_start: Option<f32>,
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
fn geometry(site: &EffectSite<'_>) -> ([f32; 2], f32) {
    let [x, y, w, h] = site.instance.rect;
    let side = w.min(h);
    ([x + w / 2.0, y + h / 2.0], side * (0.5 - 1.0 / 512.0))
}

fn span(x: f32, from: f32, to: f32) -> f32 {
    ((x - from) / (to - from)).clamp(0.0, 1.0)
}

/// The landing page's `ease`: a smoothstep.
fn ease(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

/// Where the door is in the opening: the site's sequence, in the same spans and with the same
/// easing. `rest` is the door's centre in points (x below zero: the window's middle), `radius` its
/// resting radius, `window` the frame size in device pixels, and `progress` runs from 0 to 1.
struct Sequence {
    center: [f32; 2],
    radius: f32,
    split: f32,
    spin: f32,
    gain: f32,
}

fn sequence(
    rest: [f32; 2],
    radius: f32,
    window: [u32; 2],
    scale: f32,
    progress: f32,
    reduced: bool,
) -> Option<Sequence> {
    // Reduced motion: the page simply appears; there is no sequence to draw.
    if reduced || progress >= 1.0 {
        return None;
    }
    let win = [window[0] as f32 / scale, window[1] as f32 / scale];
    let middle = [win[0] / 2.0, win[1] / 2.0];
    let rest = if rest[0] < 0.0 { middle } else { rest };
    let push = ease(span(progress, 0.03, 0.44));
    let part = ease(span(progress, 0.44, 0.82));
    let open = (win[0] * win[0] + win[1] * win[1]).sqrt() * 0.56;
    Some(Sequence {
        center: [
            (rest[0] + (middle[0] - rest[0]) * push) * scale,
            (rest[1] + (middle[1] - rest[1]) * push) * scale,
        ],
        radius: (radius + (open - radius) * push) * scale,
        split: part * 1.08,
        spin: ease(span(progress, 0.0, 0.44)) * OPENING_SPIN,
        gain: 1.0 - 0.62 * push,
    })
}

impl Compositor<State> for Gfx {
    fn animating(&self) -> bool {
        // Only while a light, a tumbler or the opening is still moving, so a window at rest draws
        // none.
        self.wake || self.opening || self.doors.values().any(Door::moving)
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
        let (t, reduced, pointer) = (f.rt.t, f.rt.reduced_motion, f.rt.pointer());
        let frame = FrameFacts {
            t,
            reduced,
            pointer,
        };
        let mut seen: HashSet<u32> = HashSet::new();
        let mut opening = false;
        let mut slot = 0usize;
        let clock = f.rt.clock;
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, f.size);
        f.ui.draw_with(f.encoder, f.target, clear, &mut |pass, site| {
            let at = if site.mark == OPEN {
                // The state starts the opening on the logical clock; the animation runs on the
                // animation clock. Convert once, so a host that slept before this frame cannot
                // leave the door stuck at rest.
                let start = *self
                    .opening_start
                    .get_or_insert_with(|| (t - (clock - site.data[0] as f64)) as f32);
                let progress = (t as f32 - start) / OPENING;
                let Some(at) = sequence(
                    [site.data[1], site.data[2]],
                    site.data[3],
                    [f.size.0, f.size.1],
                    site.scale,
                    progress,
                    reduced,
                ) else {
                    return;
                };
                opening = true;
                seen.insert(OPEN);
                let door = self.doors.entry(OPEN).or_insert_with(|| Door::new(0.0, t));
                door.step(frame, site.scale, at.center, at.radius, 0.0);
                Slot {
                    center: at.center,
                    radius: at.radius,
                    split: at.split,
                    light: door.light,
                    spin: at.spin,
                    gain: at.gain,
                    origin: site.origin,
                    encode: if site.srgb_blend { 1.0 } else { 0.0 },
                }
            } else {
                if !matches!(site.mark, DOOR | MARK | PULL) {
                    return;
                }
                let (center, radius) = geometry(site);
                let door = self
                    .doors
                    .entry(site.mark)
                    .or_insert_with(|| Door::new(site.data[0], t));
                door.step(frame, site.scale, center, radius, site.data[0]);
                seen.insert(site.mark);
                Slot {
                    center,
                    radius,
                    split: 0.0,
                    light: door.light,
                    spin: door.spin * TAU,
                    gain: 1.0,
                    origin: site.origin,
                    encode: if site.srgb_blend { 1.0 } else { 0.0 },
                }
            };
            if slot >= MAX_SLOTS {
                return;
            }
            let offset = slot as u64 * gpu.stride as u64;
            queue.write_buffer(&gpu.uniforms, offset, &at.bytes());
            slot += 1;
            pass.set_pipeline(gpu.pipeline(device, site.format));
            pass.set_bind_group(0, &gpu.bind, &[(slot as u32 - 1) * gpu.stride]);
            pass.draw(0..3, 0..1);
        });
        self.opening = opening;
        if !opening {
            self.opening_start = None;
        }
        self.doors.retain(|mark, _| seen.contains(mark));
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
    // The cut between the halves is a hairline of the whole door's width. On a mark a few tens of
    // pixels across it would be a black pixel, so it closes up there.
    let cut_keep = clamp(u.radius / 200.0, 0.0, 1.0);
    let inside = smoothstep(-px, px, p.x * side - px * 0.75 * cut_keep);
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
    // A mark is only tens of pixels across, and its lathe marks are finer than a pixel there:
    // average them into plain steel, as the picture's mipmaps do.
    let grain = smoothstep(50.0, 140.0, u.radius);
    let lines = mix(
        0.5,
        noise(r * detail + seed) * 0.6 + noise(r * detail * 0.31 + seed) * 0.4,
        grain,
    );
    let blotch = noise(turned * 3.0 + seed) * 0.5 + noise(turned * 11.0 + seed * 2.0) * 0.5;

    // The rim of each ring catches light like a chamfer, and fades as it goes under a pixel.
    let width = 0.016;
    let in_dist = r - a;
    let out_dist = b - r;
    let chamfer = (smoothstep(width, 0.0, out_dist) - smoothstep(width, 0.0, in_dist))
        * clamp(width / (px * 1.5), 0.0, 1.0);
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

    // The groove between rings: one pixel wide on a small mark, so it softens and then goes.
    let groove_w = max(0.007, px);
    let groove = smoothstep(0.0, groove_w, min(in_dist, out_dist));
    c *= mix(1.0, mix(0.16, 1.0, groove), clamp(0.007 / (px * 1.5), 0.0, 1.0));

    // The dial: graduations cut into the fourth ring, turning with it.
    if (ring == 3) {
        let count = 72.0;
        let cell = turned / 6.2831853 * count;
        let tick = abs(fract(cell) - 0.5) * 2.0;
        let major = glsl_mod(floor(cell), 6.0) < 0.5;
        let reach = select(0.026, 0.05, major);
        // Pixels per unit of `tick`, so a graduation narrower than a pixel averages away.
        let tick_px = 144.0 * px / (6.2831853 * max(r, px));
        let aa = clamp(tick_px, 0.0, 0.5);
        let present = clamp(0.06 / tick_px, 0.0, 1.0);
        let cut = smoothstep(0.9 - aa, 0.96 + aa, tick)
            * step(b - 0.018 - reach, r) * step(r, b - 0.018)
            * present
            * clamp(reach / (px * 2.0), 0.0, 1.0);
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
        // A bolt is a pixel and a half across on a mark: give it coverage, not an edge.
        let soft = clamp(px / 0.024, 0.0, 0.5);
        if (d < 1.0 + soft) {
            let dome = sqrt(max(1.0 - d * d, 0.0));
            let bolt_normal = normalize(vec3f(local / 0.024, dome + 0.2));
            let bolt_light = pow(max(dot(bolt_normal, halfway), 0.0), 26.0);
            let lit = base * 0.5 + base * 0.7 * max(dot(bolt_normal, to_light), 0.0)
                + steel * bolt_light * 0.9;
            // A bolt a pixel across is a hint, not a hole.
            let presence = clamp(0.016 / px, 0.0, 1.0);
            c = mix(c, mix(c, lit, presence), smoothstep(1.0 + soft, 1.0 - soft, d));
        }
        let ring_keep = clamp(0.0048 / px, 0.0, 1.0);
        c *= mix(
            1.0,
            mix(0.55, 1.0, smoothstep(0.92, 1.12, d)),
            ring_keep,
        );
    }

    // The cut faces of the two halves, a hairline that softens on a small mark.
    let face_dist = abs(p.x);
    c += steel * smoothstep(0.012, 0.0, face_dist) * 0.14 * smoothstep(0.0, 0.02, u.split);
    c *= mix(
        1.0,
        mix(0.25, 1.0, smoothstep(0.0, max(0.0035, px), face_dist)),
        clamp(0.0035 / (px * 1.5), 0.0, 1.0),
    );

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

    fn facts(t: f64) -> FrameFacts {
        FrameFacts {
            t,
            reduced: false,
            pointer: None,
        }
    }

    #[test]
    fn a_door_at_rest_does_not_move() {
        let mut door = Door::new(0.0, 0.0);
        for k in 0..60 {
            door.step(facts(k as f64 / 60.0), 2.0, [50.0, 50.0], 25.0, 0.0);
        }
        assert!(!door.moving());
    }

    #[test]
    fn the_light_follows_the_pointer_and_rests_up_and_left_without_one() {
        let mut door = Door::new(0.0, 0.0);
        let pointer = FrameFacts {
            pointer: Some((500.0, 500.0)),
            ..facts(1.0 / 60.0)
        };
        // Far past the reach, the light stops two radii out.
        door.step(pointer, 1.0, [50.0, 50.0], 25.0, 0.0);
        assert_eq!(door.target, [2.0, 2.0]);
        assert!(door.moving());
        for k in 2..240 {
            let at = FrameFacts {
                pointer: Some((500.0, 500.0)),
                ..facts(k as f64 / 60.0)
            };
            door.step(at, 1.0, [50.0, 50.0], 25.0, 0.0);
        }
        assert!((door.light[0] - 2.0).abs() < 0.01 && (door.light[1] - 2.0).abs() < 0.01);
        // The pointer leaves: it goes back to the page's resting light.
        door.step(facts(5.0), 1.0, [50.0, 50.0], 25.0, 0.0);
        assert_eq!(door.target, LIGHT);
    }

    #[test]
    fn a_new_door_turns_its_tumblers_once_and_settles() {
        let mut door = Door::new(2.0, 0.0);
        assert!(door.moving(), "it starts from rest with two turns to go");
        for k in 1..=180 {
            door.step(facts(k as f64 / 60.0), 2.0, [50.0, 50.0], 25.0, 2.0);
        }
        assert!((door.spin - 2.0).abs() < 0.01, "it turned: {}", door.spin);
        assert!(!door.moving(), "and came to rest");
    }

    #[test]
    fn reduced_motion_lands_the_door_at_once() {
        let mut door = Door::new(1.0, 0.0);
        let frame = FrameFacts {
            t: 1.0,
            reduced: true,
            pointer: Some((500.0, 500.0)),
        };
        door.step(frame, 1.0, [50.0, 50.0], 25.0, 3.0);
        assert_eq!(door.spin, 3.0);
        assert_eq!(door.light, [2.0, 2.0]);
        assert!(!door.moving());
    }

    #[test]
    fn the_opening_keeps_the_pages_spans() {
        // At the window's middle (x below zero), a 168-point door.
        let (rest, radius) = ([-1.0, -1.0], 84.0);
        let window = [1320, 860];
        let at = sequence(rest, radius, window, 1.0, 0.0, false).expect("the opening starts");
        assert!((at.center[0] - 660.0).abs() < 0.01);
        assert!((at.center[1] - 430.0).abs() < 0.01);
        assert!((at.radius - 84.0).abs() < 0.01);
        assert_eq!((at.split, at.spin, at.gain), (0.0, 0.0, 1.0));

        // Push runs 3% to 44%: at 44% the door fills the window and the halves have not moved.
        let open = (1320f32 * 1320.0 + 860.0 * 860.0).sqrt() * 0.56;
        let pushed = sequence(rest, radius, window, 1.0, 0.44, false).expect("still opening");
        assert!((pushed.radius - open).abs() < 0.5, "{}", pushed.radius);
        assert!((pushed.gain - 0.38).abs() < 0.01);
        assert!(pushed.split < 0.01);
        assert!((pushed.spin - 2.4).abs() < 0.001);

        // Part runs 44% to 82%, and by the end there is no door left to draw.
        let parted = sequence(rest, radius, window, 1.0, 0.82, false).expect("still opening");
        assert!((parted.split - 1.08).abs() < 0.001);
        assert!(sequence(rest, radius, window, 1.0, 1.0, false).is_none());
        // Reduced motion skips it: the page is simply there.
        assert!(sequence(rest, radius, window, 1.0, 0.5, true).is_none());
    }
}
