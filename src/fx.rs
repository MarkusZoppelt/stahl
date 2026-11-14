//! The room: its one light, the vault door, and the light's effects on steel and glass.
//!
//! The door is a port of the landing page's fragment shader (`site/door.js`) to WGSL, run
//! through Rune's compositor hook. Views place an effect slot with [`door`], [`mark`], [`pull`]
//! or [`opening`]; the compositor paints the steel there, at whatever size and place the slot
//! has, in the window and in a headless render alike.
//!
//! Everything else the compositor draws hangs off the same light: [`room`] is the soft pool of
//! light on the wall behind a page, and the door's steel is lit from it, so a door and the wall
//! it stands on agree about where the light is. A little later the light also catches the
//! figure ([`sheen`]) and the edge of a pressable surface ([`lift`]).
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
    render::{EffectPhase, EffectSite, KIND_EFFECT},
};
use rune_fx::{Fx, FxFrame, FxLayer, FxOptions, FxRenderer, FxSlot, LensParams, Shape, rgb};

/// The large door: the welcome screen's, and the one that opens a portfolio.
pub const DOOR: u32 = 1;
/// The small mark: the sidebar's, the phone's top bar.
pub const MARK: u32 = 2;
/// The door behind a page pulled past its top on a phone.
pub const PULL: u32 = 3;
/// The full-window slot that plays the opening when a portfolio first shows.
pub const OPEN: u32 = 4;
/// The room's light on the wall behind a page.
pub const ROOM: u32 = 5;
/// The steel sheen over a figure.
pub const SHEEN: u32 = 6;
/// The faint glow on the edge of a pressable surface nearest the pointer.
pub const LIFT: u32 = 7;
/// A floating panel's glass.
pub const GLASS: u32 = 8;

/// How long the refresh's pulse takes to cross the page.
pub const PULSE_LIFE: f64 = 0.95;
/// How long light runs along the total for a new record.
pub const FLOURISH: f64 = 1.3;

/// The uniform every slot of the door and room shaders writes, in bytes: the door's own
/// 48-byte struct and the room's larger one share one buffer and one stride.
const UNIFORM_SIZE: usize = 128;
const MAX_SLOTS: usize = 64;

/// How long the opening takes, as on the landing page.
const OPENING: f32 = 1.8;
/// How far the tumblers turn while the door pushes in.
const OPENING_SPIN: f32 = 2.4;

/// How heavy the tumblers are: a turn settles in about a second, with a small give.
const SPRING: f32 = 70.0;
const DAMPING: f32 = 10.0;

/// The light follows a pointer within two radii of the door, as the page's does.
const LIGHT_REACH: f32 = 2.0;
/// How far off the steel's face the light sits at its furthest, in door radii: the landing
/// page's resting light, so the steel keeps the elevation it was drawn with.
const LIGHT_TILT: f32 = 0.9;

/// How long the tumblers take for one slow turn while something is being thought about.
const THINKING_TURN: f32 = 9.0;

/// The light's colour when nothing has moved: steel blue.
const STEEL: [f32; 3] = [0.72, 0.82, 0.92];
/// The faintest warm teal, for a day that is up.
const TEAL: [f32; 3] = [0.66, 0.88, 0.80];
/// The faintest red, for a day that is down.
const RED: [f32; 3] = [0.94, 0.74, 0.71];

/// A slot of `size` points that the compositor paints the door into. It draws nothing itself:
/// the effect keeps the slot's place, rect, clip and opacity. `turns` is how far the tumblers
/// have been turned, and `busy` keeps them turning slowly; the compositor eases the real spin
/// towards it.
fn slot(size: f32, mark: u32, turns: f32, busy: bool, hand: f32, dragging: bool) -> El {
    el().size(size, size)
        .effect(mark)
        .effect_data([
            turns,
            if busy { 1.0 } else { 0.0 },
            hand,
            if dragging { 1.0 } else { 0.0 },
        ])
        .a11y_hidden()
}

/// The large door, at `size`.
pub fn door(size: f32, turns: u64, busy: bool, hand: f32, dragging: bool) -> El {
    slot(size, DOOR, turns as f32, busy, hand, dragging)
}

/// The small mark, at `size`.
pub fn mark(size: f32, turns: u64, busy: bool, hand: f32, dragging: bool) -> El {
    slot(size, MARK, turns as f32, busy, hand, dragging)
}

/// The door behind a pull, at `size`. It never turns.
pub fn pull(size: f32) -> El {
    slot(size, PULL, 0.0, false, 0.0, false)
}

/// The room's light on the wall, filling its parent. `radius` is the parent's corner radius, so
/// the light is cut to the same rounded pane as the page it falls on. `pulse` is
/// `[started, x, y, 1]` while a refresh's pulse crosses the page, or zeros.
pub fn room(radius: f32, pulse: [f32; 4]) -> El {
    el().absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .radius(radius)
        .effect(ROOM)
        .effect_data(pulse)
        .a11y_hidden()
        .transition(Transition::none())
}

/// The steel sheen over a figure: a soft band of light whose place follows the room's light.
/// `strength` is how much of the effect to show; `run` is 0 when still, or 0 to 1 while a new
/// record sends one line of light along the metal.
pub fn sheen(strength: f32, run: f32) -> El {
    el().absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .effect(SHEEN)
        .effect_backdrop(true)
        .effect_data([strength, run, 0.0, 0.0])
        .a11y_hidden()
        .transition(Transition::none())
}

/// A pressable surface's edge catching the light: a faint glow on the side nearest the pointer.
/// `radius` is the surface's corner radius, so the glow follows a rounded edge.
pub fn lift(radius: f32) -> El {
    el().absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .radius(radius)
        .effect(LIFT)
        .a11y_hidden()
        .transition(Transition::none())
}

/// The glass a floating panel is made of: a lens over what lies behind it, frosted and lit at
/// the rim facing the room's light. The panel's own content is drawn over it by the caller.
pub fn glass() -> El {
    el().absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .radius(12.0)
        .effect(GLASS)
        .effect_backdrop(true)
        .a11y_hidden()
        .transition(Transition::none())
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
    /// The pointer in window points.
    pointer: Option<(f32, f32)>,
    /// The window in points.
    window: [f32; 2],
    dark: bool,
    /// Today's move in percent, if the portfolio has one.
    day: Option<f64>,
}

/// The one light in the room, in window points. It follows the pointer slowly and softly and
/// rests above the middle of the window when nobody steers. Its colour carries the day.
#[derive(Clone, Copy)]
struct Light {
    at: [f32; 2],
    target: [f32; 2],
    velocity: [f32; 2],
    tint: [f32; 3],
    tint_target: [f32; 3],
    /// How large the day's move is, 0 to 1, capped low in the shader.
    strength: f32,
    strength_target: f32,
    t: f64,
}

/// How heavy the room's light is: a critically damped spring, so it lags the pointer and comes
/// to rest in about two seconds without overshooting.
const ROOM_SPRING: f32 = 36.0;
const ROOM_DAMPING: f32 = 12.0;
/// How fast its colour takes the day, a frame at 60.
const TINT_EASE: f32 = 0.06;

/// The light's colour and the strength of the day behind it: steel blue when nothing moved, the
/// faintest teal up and the faintest red down, growing with the size of the move.
fn day_light(day: Option<f64>) -> ([f32; 3], f32) {
    let Some(day) = day.filter(|day| day.is_finite()) else {
        return (STEEL, 0.0);
    };
    let tint = if day > 0.005 {
        TEAL
    } else if day < -0.005 {
        RED
    } else {
        STEEL
    };
    (tint, (day.abs() / 2.0).min(1.0) as f32)
}

impl Light {
    /// A light seen for the first time is already where it belongs, so the first frame does not
    /// sweep it in from a corner.
    fn new(frame: FrameFacts) -> Light {
        let at = match frame.pointer {
            Some((x, y)) => [x, y],
            None => Light::rest(frame.window),
        };
        let (tint, strength) = day_light(frame.day);
        Light {
            at,
            target: at,
            velocity: [0.0, 0.0],
            tint,
            tint_target: tint,
            strength,
            strength_target: strength,
            t: frame.t,
        }
    }

    /// Where the light rests with nobody steering: above the top of the window, in the middle.
    fn rest(window: [f32; 2]) -> [f32; 2] {
        [window[0] * 0.5, -window[1] * 0.22]
    }

    fn step(&mut self, frame: FrameFacts) {
        if frame.t <= self.t {
            return;
        }
        let dt = ((frame.t - self.t) as f32).min(0.05);
        self.t = frame.t;
        self.target = match frame.pointer {
            Some((x, y)) => [x, y],
            None => Light::rest(frame.window),
        };
        (self.tint_target, self.strength_target) = day_light(frame.day);
        if frame.reduced {
            self.at = self.target;
            self.velocity = [0.0, 0.0];
            self.tint = self.tint_target;
            self.strength = self.strength_target;
            return;
        }
        // A heavy, softly damped settle, in fixed steps so a long frame can't blow it up.
        let mut left = dt;
        while left > 0.0 {
            let h = left.min(1.0 / 240.0);
            for axis in 0..2 {
                let pull = -ROOM_SPRING * (self.at[axis] - self.target[axis])
                    - ROOM_DAMPING * self.velocity[axis];
                self.velocity[axis] += pull * h;
                self.at[axis] += self.velocity[axis] * h;
            }
            left -= h;
        }
        let close = (0..2).all(|axis| (self.at[axis] - self.target[axis]).abs() < 0.05)
            && (0..2).all(|axis| self.velocity[axis].abs() < 0.1);
        if close {
            self.at = self.target;
            self.velocity = [0.0, 0.0];
        }
        // The day's colour follows without a spring: it is a meaning, not a movement.
        let k = 1.0 - (1.0 - TINT_EASE).powf(dt * 60.0);
        for channel in 0..3 {
            self.tint[channel] += (self.tint_target[channel] - self.tint[channel]) * k;
            if (self.tint[channel] - self.tint_target[channel]).abs() < 0.01 {
                self.tint[channel] = self.tint_target[channel];
            }
        }
        self.strength += (self.strength_target - self.strength) * k;
        if (self.strength - self.strength_target).abs() < 0.01 {
            self.strength = self.strength_target;
        }
    }

    fn moving(&self) -> bool {
        self.at != self.target
            || self.tint != self.tint_target
            || self.strength != self.strength_target
    }
}

/// One door's live state: how far its tumblers have turned, and whether they are turning on
/// their own. The compositor keeps this, so a moving light never rebuilds the view.
struct Door {
    /// Turns, eased; the shader wants radians.
    spin: f32,
    spin_target: f32,
    speed: f32,
    /// The last turn count the view asked for.
    turns: f32,
    /// Something is being thought about: the tumblers turn slowly until it isn't.
    busy: bool,
    /// How far the owner has turned the dial by hand, in radians.
    hand: f32,
    /// How fast the hand was turning when the drag ended, to coast on.
    hand_speed: f32,
    dragging: bool,
    /// The clock when this door last stepped, so a frame that draws it twice advances it once.
    t: f64,
}

impl Door {
    /// A door seen for the first time starts its tumblers at rest and turns them to `turns`, as
    /// a freshly built dial did, so a refresh shown by a new mark still turns.
    fn new(turns: f32, t: f64) -> Door {
        Door {
            spin: 0.0,
            spin_target: turns,
            speed: 0.0,
            turns,
            busy: false,
            hand: 0.0,
            hand_speed: 0.0,
            dragging: false,
            t,
        }
    }

    fn step(&mut self, t: f64, reduced: bool, turns: f32, busy: bool, hand: f32, dragging: bool) {
        if t <= self.t {
            return;
        }
        let dt = ((t - self.t) as f32).min(0.05);
        self.t = t;
        if turns != self.turns {
            self.turns = turns;
            if !dragging {
                self.spin_target = turns + hand;
            }
        }
        // A hand on the dial adds its angle to the turn count; letting go carries its speed, so
        // the steel coasts past and settles back with the same heavy spring.
        if dragging && dt > 0.0 {
            self.hand_speed = (hand - self.hand) / dt;
        }
        if self.dragging && !dragging {
            self.speed += self.hand_speed.clamp(-12.0, 12.0) * 0.8;
        }
        self.hand = hand;
        self.dragging = dragging;
        if busy != self.busy {
            self.busy = busy;
            if busy {
                // The turn starts from wherever the steel is, so it never jumps.
                self.spin_target = self.spin;
            } else {
                // It settles on a whole turn, with the same heavy spring as after a refresh.
                self.spin_target = (self.spin / TAU).round() * TAU;
            }
        }
        if self.busy {
            // A slow, even turn, with no spring to fight it.
            self.spin_target += dt / THINKING_TURN * TAU;
        } else if !dragging {
            self.spin_target = turns + hand;
        }
        if reduced {
            self.spin = self.spin_target;
            self.speed = 0.0;
            return;
        }
        // A heavy, slightly sprung settle, in fixed steps so a long frame can't blow it up.
        let mut left = dt;
        while left > 0.0 {
            let h = left.min(1.0 / 240.0);
            let pull = -SPRING * (self.spin - self.spin_target) - DAMPING * self.speed;
            self.speed += pull * h;
            self.spin += self.speed * h;
            left -= h;
        }
        if !self.busy
            && !self.dragging
            && (self.spin - self.spin_target).abs() < 1e-4
            && self.speed.abs() < 1e-2
        {
            self.spin = self.spin_target;
            self.speed = 0.0;
        }
    }

    fn moving(&self) -> bool {
        self.busy
            || self.dragging
            || (self.spin - self.spin_target).abs() > 5e-4
            || self.speed.abs() > 1e-2
    }
}

/// The compositor: Rune draws the frame, and this paints the room and the door at each slot.
#[derive(Default)]
pub struct Gfx {
    gpu: Option<Gpu>,
    doors: HashMap<u32, Door>,
    /// The one light in the room. Every door is lit from it.
    light: Option<Light>,
    /// Rune's effect layers (glass, glows, one-shots), on the same compositor as the door.
    fx: Option<(rune::compose::DeviceKey, FxRenderer)>,
    /// How far each pressable surface has lifted toward the pointer, by slot, so the glow eases
    /// in and out instead of popping.
    lifts: HashMap<u32, f32>,
    /// The frame that first found the compositor, whose view still shows the picture: draw one
    /// more so the door appears without waiting for the owner to touch something.
    wake: bool,
    /// The opening was drawn this frame and has not finished: keep the display link running.
    opening: bool,
    /// When the opening started on the animation clock, so its progress and the state's timer
    /// share one clock even after the host slept.
    opening_start: Option<f32>,
}

/// Which shader a pipeline draws with.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Door,
    Room,
    Sheen,
}

struct Gpu {
    key: rune::compose::DeviceKey,
    layout: wgpu::PipelineLayout,
    door: wgpu::ShaderModule,
    room: wgpu::ShaderModule,
    sheen: wgpu::ShaderModule,
    /// The sheen reads the backdrop, so it binds a texture and a sampler beside the uniform.
    sheen_layout: wgpu::PipelineLayout,
    sheen_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipes: HashMap<(Kind, wgpu::TextureFormat), wgpu::RenderPipeline>,
    uniforms: wgpu::Buffer,
    bind: wgpu::BindGroup,
    /// Bytes between one slot's uniforms and the next: the adapter's offset alignment, which is
    /// larger than the bytes themselves.
    stride: u32,
}

impl Gpu {
    fn new(device: &wgpu::Device, key: rune::compose::DeviceKey) -> Gpu {
        let door = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("door"),
            source: wgpu::ShaderSource::Wgsl(DOOR_WGSL.into()),
        });
        let room = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("room"),
            source: wgpu::ShaderSource::Wgsl(ROOM_WGSL.into()),
        });
        let sheen = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sheen"),
            source: wgpu::ShaderSource::Wgsl(SHEEN_WGSL.into()),
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
        let sheen_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sheen"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(UNIFORM_SIZE as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("door"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let sheen_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sheen"),
            bind_group_layouts: &[Some(&sheen_bgl)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sheen"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
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
            door,
            room,
            sheen,
            sheen_layout,
            sheen_bgl,
            sampler,
            pipes: HashMap::new(),
            uniforms,
            bind,
            stride,
        }
    }

    fn pipeline(
        &mut self,
        device: &wgpu::Device,
        kind: Kind,
        format: wgpu::TextureFormat,
    ) -> &wgpu::RenderPipeline {
        let module = match kind {
            Kind::Door => &self.door,
            Kind::Room => &self.room,
            Kind::Sheen => &self.sheen,
        };
        let layout = match kind {
            Kind::Sheen => &self.sheen_layout,
            _ => &self.layout,
        };
        self.pipes.entry((kind, format)).or_insert_with(|| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("door"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
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

/// One slot's uniforms, laid out as `U` in the door shader and `R` in the room shader.
struct Slot {
    center: [f32; 2],
    radius: f32,
    split: f32,
    light: [f32; 2],
    spin: f32,
    gain: f32,
    origin: [f32; 2],
    encode: f32,
    /// Which of the room shader's branches to draw: 1 is the room's light, 3 a surface's edge.
    mode: f32,
    window: [f32; 2],
    /// The slot's rect in points.
    rect: [f32; 4],
    /// The light's tint, and how much of it.
    color: [f32; 4],
    /// x: the dark theme, y: the pointer is over the surface, z/w: the branch's own numbers.
    flags: [f32; 4],
    /// Device pixels per point at the slot.
    scale: f32,
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
            self.mode,
            self.window[0],
            self.window[1],
            self.scale,
            0.0,
            self.rect[0],
            self.rect[1],
            self.rect[2],
            self.rect[3],
            self.color[0],
            self.color[1],
            self.color[2],
            self.color[3],
            self.flags[0],
            self.flags[1],
            self.flags[2],
            self.flags[3],
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
        // Only while the light, a tumbler or the opening is still moving, so a window at rest
        // draws none.
        self.wake
            || self.opening
            || self.light.as_ref().is_some_and(Light::moving)
            || self.doors.values().any(Door::moving)
            // A surface's glow is still easing in or out.
            || self
                .lifts
                .values()
                .any(|lift| *lift > 0.001 && *lift < 0.999)
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
        let window = [f.rt.viewport.0, f.rt.viewport.1];
        let day =
            f.rt.state
                .summary
                .as_ref()
                .map(|summary| summary.day_change_percent);
        let frame = FrameFacts {
            t,
            reduced,
            pointer,
            window,
            dark: f.rt.state.is_dark(),
            day,
        };
        let light = match &mut self.light {
            Some(light) => {
                light.step(frame);
                *light
            }
            None => {
                let light = Light::new(frame);
                self.light = Some(light);
                light
            }
        };
        let (tint, strength) = (light.tint, light.strength);
        let dark = frame.dark;
        // Every floating panel's glass is one rune-fx lens, lit from the room's light.
        let mut layers: Vec<FxLayer> = Vec::new();
        let mut placements: Vec<FxSlot> = Vec::new();
        let mut layer_at: HashMap<u32, usize> = HashMap::new();
        for (n, i) in f.rt.instances.iter().enumerate() {
            if i.params[1] != KIND_EFFECT {
                continue;
            }
            if f.rt.marks.get(n).copied().unwrap_or(0) != GLASS {
                continue;
            }
            let k = i.params[3];
            let [x, y, w, h] = i.rect.map(|v| v / k);
            let radius = i.radii.iter().fold(0.0f32, |a, v| a.max(v.abs())) / k;
            let centre = [x + w / 2.0, y + h / 2.0];
            let angle = (light.at[1] - centre[1]).atan2(light.at[0] - centre[0]);
            // A small panel gets a small bevel: a 46-point readout must not be mostly rim.
            let side = w.min(h);
            let params = LensParams {
                bevel: (side * 0.18).clamp(6.0, 16.0),
                depth: (side * 0.24).clamp(8.0, 20.0),
                dispersion: 0.10,
                magnification: 1.02,
                presence: 1.0,
                frost: 0.8,
                // The panel's own shadow is Rune's; the lens only lights its edge.
                shadow: 0.0,
                rim: 1.3,
                light_angle: angle,
                tint: if dark { rgb(0x000000) } else { rgb(0xffffff) },
                tint_amount: 0.0,
                ..Default::default()
            };
            layer_at.insert(n as u32, layers.len());
            layers.push(
                Fx::lens(params)
                    .on(Shape::rounded(x, y, w, h, radius))
                    .at(t as f32),
            );
            placements.push(FxSlot::default().scale(k).clip(i.clip, i.fade));
        }
        if self.fx.as_ref().is_none_or(|(made, _)| *made != key) {
            let mut fx = FxRenderer::new(device, f.format, FxOptions { dither: true });
            fx.add_format(device, wgpu::TextureFormat::Rgba16Float);
            self.fx = Some((key, fx));
        }
        let fx = &mut self.fx.as_mut().expect("just made").1;
        fx.set_srgb_blend(f.srgb_blend());
        if !layers.is_empty() {
            fx.prepare_with_slots(
                device,
                queue,
                &FxFrame {
                    viewport: [f.size.0, f.size.1],
                    scale: 1.0,
                    layers: &layers,
                    backdrop: None,
                    sources: &[],
                },
                &placements,
            );
        }
        let mut seen: HashSet<u32> = HashSet::new();
        let mut lift_seen: HashSet<u32> = HashSet::new();
        let mut opening = false;
        let mut slot = 0usize;
        let clock = f.rt.clock;
        let dt = f.dt.max(1.0 / 240.0) as f32;
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, f.size);
        f.ui.draw_with_backdrop(f.encoder, f.target, clear, &mut |phase, site| {
            if let Some(&index) = layer_at.get(&site.index) {
                match phase {
                    EffectPhase::Backdrop(encoder) => {
                        if let Some(backdrop) = site.backdrop {
                            fx.blur_backdrop(device, queue, encoder, backdrop, index);
                        }
                    }
                    EffectPhase::Draw(pass) => {
                        fx.draw_layer_in(pass, site.format, site.origin, index);
                    }
                }
                return;
            }
            let EffectPhase::Draw(pass) = phase else {
                return;
            };
            let encode = if site.srgb_blend { 1.0 } else { 0.0 };
            let rect = {
                let [x, y, w, h] = site.instance.rect;
                let scale = site.scale.max(1e-3);
                [x / scale, y / scale, w / scale, h / scale]
            };
            let at = if site.mark == ROOM {
                seen.insert(ROOM);
                // A refresh's pulse, while it runs, in the same pass as the room's light.
                let progress = if site.data[3] > 0.5 && !reduced {
                    let start = t - (clock - site.data[0] as f64);
                    ((t - start) / PULSE_LIFE) as f32
                } else {
                    2.0
                };
                Slot {
                    center: [site.data[1], site.data[2]],
                    radius: 0.0,
                    split: 0.0,
                    light: light.at,
                    spin: progress,
                    gain: strength,
                    origin: site.origin,
                    encode,
                    mode: 1.0,
                    window,
                    rect,
                    color: [tint[0], tint[1], tint[2], 0.0],
                    flags: [if dark { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
                    scale: site.scale,
                }
            } else if site.mark == LIFT {
                seen.insert(LIFT);
                lift_seen.insert(site.index);
                // The glow eases in as the pointer arrives and out as it leaves.
                let [rx, ry, rw, rh] = site.instance.rect;
                let inside = pointer.is_some_and(|(x, y)| {
                    let (x, y) = (x * site.scale, y * site.scale);
                    x >= rx && x <= rx + rw && y >= ry && y <= ry + rh
                });
                let ease = if reduced {
                    1.0
                } else {
                    1.0 - 0.82f32.powf(dt * 60.0)
                };
                let lift = self.lifts.entry(site.index).or_insert(0.0);
                *lift += ((if inside { 1.0 } else { 0.0 }) - *lift) * ease;
                if *lift < 0.001 {
                    *lift = 0.0;
                } else if *lift > 0.999 {
                    *lift = 1.0;
                }
                let lift = *lift;
                let radius = site
                    .instance
                    .radii
                    .iter()
                    .fold(0.0f32, |a, v| a.max(v.abs()))
                    / site.scale.max(1e-3);
                let pointer = pointer.map(|(x, y)| [x, y]).unwrap_or([f32::MIN, f32::MIN]);
                Slot {
                    center: [0.0, 0.0],
                    radius,
                    split: 0.0,
                    light: pointer,
                    spin: 0.0,
                    gain: 1.0,
                    origin: site.origin,
                    encode,
                    mode: 3.0,
                    window,
                    rect,
                    color: [0.84, 0.91, 0.97, 0.0],
                    flags: [if dark { 1.0 } else { 0.0 }, lift, 0.0, 0.0],
                    scale: site.scale,
                }
            } else if site.mark == SHEEN {
                // The sheen reads the figure it falls on, so it is keyed by the backdrop's own
                // light and dark rather than by a mask Rune does not keep for text.
                let Some(backdrop) = site.backdrop else {
                    return;
                };
                if slot >= MAX_SLOTS {
                    return;
                }
                let at = Slot {
                    center: [0.0, 0.0],
                    radius: 0.0,
                    split: 0.0,
                    light: light.at,
                    spin: 0.0,
                    gain: site.data[0],
                    origin: site.origin,
                    encode,
                    mode: 2.0,
                    window,
                    rect,
                    color: [0.84, 0.90, 0.96, 0.0],
                    flags: [
                        if dark { 1.0 } else { 0.0 },
                        0.0,
                        site.data[0],
                        // Reduced motion gets the still frame: no run of light.
                        if reduced { 0.0 } else { site.data[1] },
                    ],
                    scale: site.scale,
                };
                let offset = slot as u64 * gpu.stride as u64;
                queue.write_buffer(&gpu.uniforms, offset, &at.bytes());
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("sheen"),
                    layout: &gpu.sheen_bgl,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &gpu.uniforms,
                                offset: 0,
                                size: wgpu::BufferSize::new(UNIFORM_SIZE as u64),
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(backdrop),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&gpu.sampler),
                        },
                    ],
                });
                slot += 1;
                pass.set_pipeline(gpu.pipeline(device, Kind::Sheen, site.format));
                pass.set_bind_group(0, &bind, &[(slot as u32 - 1) * gpu.stride]);
                pass.draw(0..3, 0..1);
                return;
            } else if site.mark == OPEN {
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
                door.step(t, reduced, 0.0, false, 0.0, false);
                let light_at = [light.at[0] * site.scale, light.at[1] * site.scale];
                let direction = light_direction(light_at, at.center, at.radius);
                Slot {
                    center: at.center,
                    radius: at.radius,
                    split: at.split,
                    light: direction,
                    spin: at.spin,
                    gain: at.gain,
                    origin: site.origin,
                    encode,
                    mode: 0.0,
                    window,
                    rect,
                    color: [0.0; 4],
                    flags: [0.0; 4],
                    scale: site.scale,
                }
            } else if matches!(site.mark, DOOR | MARK | PULL) {
                let (center, radius) = geometry(site);
                let busy = site.data[1] > 0.5;
                let door = self
                    .doors
                    .entry(site.mark)
                    .or_insert_with(|| Door::new(site.data[0], t));
                door.step(
                    t,
                    reduced,
                    site.data[0],
                    busy,
                    site.data[2],
                    site.data[3] > 0.5,
                );
                // While something is being thought about, the light on the steel breathes.
                let breath = if busy && !reduced {
                    1.0 + 0.13 * (t as f32 * 1.7).sin()
                } else {
                    1.0
                };
                seen.insert(site.mark);
                let light_at = [light.at[0] * site.scale, light.at[1] * site.scale];
                Slot {
                    center,
                    radius,
                    split: 0.0,
                    light: light_direction(light_at, center, radius),
                    spin: door.spin * TAU,
                    gain: breath,
                    origin: site.origin,
                    encode,
                    mode: 0.0,
                    window,
                    rect,
                    color: [0.0; 4],
                    flags: [0.0; 4],
                    scale: site.scale,
                }
            } else {
                return;
            };
            if slot >= MAX_SLOTS {
                return;
            }
            let offset = slot as u64 * gpu.stride as u64;
            queue.write_buffer(&gpu.uniforms, offset, &at.bytes());
            let kind = if at.mode == 0.0 {
                Kind::Door
            } else {
                Kind::Room
            };
            slot += 1;
            pass.set_pipeline(gpu.pipeline(device, kind, site.format));
            pass.set_bind_group(0, &gpu.bind, &[(slot as u32 - 1) * gpu.stride]);
            pass.draw(0..3, 0..1);
        });
        self.opening = opening;
        if !opening {
            self.opening_start = None;
        }
        self.doors.retain(|mark, _| seen.contains(mark));
        self.lifts.retain(|index, _| lift_seen.contains(index));
    }
}

/// The light's direction from `center`, in units of `radius`, saturating at two radii as the
/// landing page's canvas did. A light right on the door keeps the page's own up-and-left tilt,
/// so the steel never floods white.
fn light_direction(light: [f32; 2], center: [f32; 2], radius: f32) -> [f32; 2] {
    let d = [light[0] - center[0], light[1] - center[1]];
    let distance = (d[0] * d[0] + d[1] * d[1]).sqrt();
    let direction = if distance < 1.0 {
        // The landing page's own resting light, as a unit vector.
        [-0.646, -0.764]
    } else {
        [d[0] / distance, d[1] / distance]
    };
    let magnitude = ((distance / radius.max(1.0)).min(LIGHT_REACH) * (LIGHT_TILT / LIGHT_REACH))
        .max(LIGHT_TILT * 0.5);
    [direction[0] * magnitude, direction[1] * magnitude]
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

/// The room's light on the wall, and the edge of a pressable surface catching it. One module
/// with a `mode` switch: 1 is the light behind a page, 3 the glow on a surface nearest the
/// pointer. Colours are authored in sRGB like the door's; the output is premultiplied and
/// encoded for whichever target the pass draws into.
const ROOM_WGSL: &str = r#"
struct R {
    center: vec2f,
    radius: f32,
    split: f32,
    light: vec2f,
    spin: f32,
    gain: f32,
    origin: vec2f,
    encode: f32,
    mode: f32,
    window: vec2f,
    scale: f32,
    pad0: f32,
    rect: vec4f,
    color: vec4f,
    flags: vec4f,
    pad1: vec4f,
};

@group(0) @binding(0) var<uniform> u: R;

fn to_lin(c: vec3f) -> vec3f {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3f(0.0)) + 0.055) / 1.055, vec3f(2.4));
    return select(hi, lo, c <= vec3f(0.04045));
}

fn hash2(p: vec2f) -> f32 { return fract(sin(dot(p, vec2f(12.9898, 78.233))) * 43758.5453); }

fn encode_out(straight: vec3f, alpha: f32) -> vec4f {
    let encoded = select(to_lin(straight), straight, u.encode > 0.5);
    return vec4f(encoded * alpha, alpha);
}

// A signed distance to a rounded rectangle centred on the origin.
fn sd_round(p: vec2f, half: vec2f, r: f32) -> f32 {
    let q = abs(p) - half + vec2f(r);
    return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// The pool of light on the wall. `p` is in window points.
fn room(p: vec2f) -> vec4f {
    let win = u.window;
    let dark = u.flags.x > 0.5;
    // Wider than tall, so the light spreads across the page as a ceiling light would.
    let rx = max(win.x * 0.70, 460.0);
    let ry = max(win.y * 0.85, 400.0);
    let q = (p - u.light) / vec2f(rx, ry);
    let r = length(q);
    let fall = exp(-1.6 * r * r);
    let m = clamp(u.gain, 0.0, 1.0);
    var lit: f32;
    var shade: f32;
    if (dark) {
        // A faint cool pool, and the far wall a shade darker than the near one.
        lit = (0.055 + 0.040 * m) * fall;
        shade = 0.035 * smoothstep(0.7, 1.7, r);
    } else {
        // Light falling on paper: a warm lift, and a barely-there shade away from it.
        lit = (0.34 + 0.14 * m) * fall;
        shade = 0.020 * smoothstep(0.8, 1.8, r);
    }
    // A refresh's pulse: one soft ring leaving the door and crossing the wall, once.
    var ring = 0.0;
    let age = u.spin;
    if (age >= 0.0 && age < 1.0) {
        let d = length(p - u.center);
        let front = age * (win.x + win.y) * 0.62;
        let width = 80.0 + 70.0 * age;
        let fade = (1.0 - age) * (1.0 - age);
        ring = exp(-pow((d - front) / width, 2.0)) * fade * select(0.10, 0.075, dark);
    }
    // A static half-LSB dither: soft gradients must not band on an 8-bit target.
    let grain = (hash2(floor(p * u.scale)) - 0.5) / 255.0;
    let alpha = clamp(lit + shade + ring + grain, 0.0, 1.0);
    // On paper the day's colour is a hint: the light is warm white with a trace of it.
    let tint = select(mix(u.color.rgb, vec3f(1.0, 0.995, 0.98), 0.80), u.color.rgb, dark);
    let rgb = tint * lit + vec3f(0.86, 0.93, 1.0) * ring;
    if (alpha <= 0.0001) { return vec4f(0.0); }
    return encode_out(rgb / alpha, alpha);
}

// A pressable surface lifting toward the pointer: the edge nearest it catches a little light.
fn lift(p: vec2f) -> vec4f {
    let hover = u.flags.y;
    if (hover <= 0.001) { return vec4f(0.0); }
    let r = u.rect;
    let half = r.zw * 0.5;
    let centre = r.xy + half;
    let local = p - centre;
    let sd = sd_round(local, half, u.radius);
    if (sd > 1.0) { return vec4f(0.0); }
    let to_light = u.light - centre;
    let away = length(to_light);
    let reach = normalize(local + vec2f(1e-4));
    let toward = normalize(to_light + vec2f(1e-4));
    let facing = select(0.0, clamp(dot(reach, toward), 0.0, 1.0), away > 1.0);
    let glow = exp(min(sd, 0.0) / 9.0) * (0.30 + 0.70 * facing * facing);
    let dark = u.flags.x > 0.5;
    let alpha = glow * select(0.16, 0.11, dark) * hover;
    return encode_out(u.color.rgb, alpha);
}

// A refresh's pulse of light: one soft ring leaving the door and crossing the page, once.
fn pulse(p: vec2f) -> vec4f {
    let age = clamp(u.spin, 0.0, 1.0);
    let d = length(p - u.light);
    let front = age * (u.window.x + u.window.y) * 0.62;
    let width = 80.0 + 70.0 * age;
    let ring = exp(-pow((d - front) / width, 2.0));
    let fade = (1.0 - age) * (1.0 - age);
    let alpha = ring * fade * select(0.10, 0.085, u.flags.x > 0.5);
    if (alpha <= 0.0005) { return vec4f(0.0); }
    return encode_out(u.color.rgb, alpha);
}

@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let p = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    let frag = pos.xy + u.origin;
    let p = frag / u.scale;
    if (u.mode > 3.5) {
        return pulse(p);
    }
    if (u.mode > 2.0) {
        return lift(p);
    }
    return room(p);
}
"#;

/// The steel sheen on a figure: a soft band of light whose place follows the room's light. The
/// figure itself is the mask, read from the backdrop the slot asked for, so the digits stay the
/// text Rune drew (they still roll, and stay crisp); the band only shades what is already there.
const SHEEN_WGSL: &str = r#"
struct S {
    center: vec2f,
    radius: f32,
    split: f32,
    light: vec2f,
    spin: f32,
    gain: f32,
    origin: vec2f,
    encode: f32,
    mode: f32,
    window: vec2f,
    scale: f32,
    pad0: f32,
    rect: vec4f,
    color: vec4f,
    flags: vec4f,
    pad1: vec4f,
};

@group(0) @binding(0) var<uniform> u: S;
@group(0) @binding(1) var backdrop: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

fn to_lin(c: vec3f) -> vec3f {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3f(0.0)) + 0.055) / 1.055, vec3f(2.4));
    return select(hi, lo, c <= vec3f(0.04045));
}

fn encode_out(straight: vec3f, alpha: f32) -> vec4f {
    let encoded = select(to_lin(straight), straight, u.encode > 0.5);
    return vec4f(encoded * alpha, alpha);
}

@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let p = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    let size = vec2f(textureDimensions(backdrop));
    let under = textureSampleLevel(backdrop, samp, pos.xy / size, 0.0);
    let rgb = under.rgb / max(under.a, 0.0001);
    let lum = dot(rgb, vec3f(0.2126, 0.7152, 0.0722));
    let dark = u.flags.x > 0.5;
    // The figure is either the brightest thing in its box (dark theme) or the darkest (paper).
    let ink = select(smoothstep(0.55, 0.30, lum), smoothstep(0.30, 0.62, lum), dark);
    if (ink <= 0.001) { return vec4f(0.0); }

    let r = u.rect;
    let p = (pos.xy + u.origin) / u.scale;
    let s = (p.x - r.x) / max(r.z, 1.0);
    let v = (p.y - r.y) / max(r.w, 1.0);
    // The band runs at about a hundred degrees, as the landing page's title does.
    let across = s + (v - 0.5) * 0.34;
    // The light's place across the window, mapped onto the figure: at rest it is the middle.
    let centre = clamp(0.5 + (u.light.x / max(u.window.x, 1.0) - 0.5) * 1.1, -0.15, 1.15);
    let band = exp(-pow((across - centre) / 0.24, 2.0));

    let strength = clamp(u.flags.z, 0.0, 1.0);
    // A new record: one line of light runs along the metal, once.
    let run = u.flags.w;
    var extra_dim = 0.0;
    var extra_light = 0.0;
    if (run > 0.0 && run < 1.0) {
        let envelope = sin(run * 3.14159265);
        let sweep = exp(-pow((across - (run * 1.6 - 0.3)) / 0.14, 2.0)) * envelope;
        extra_dim = 0.15 * (1.0 - sweep) * envelope;
        extra_light = 0.45 * sweep;
    }
    if (dark) {
        // White digits have nowhere brighter to go: the rest of the steel dims a little, so the
        // band reads as the lit part.
        return encode_out(
            vec3f(0.0),
            ink * ((1.0 - band) * 0.10 * strength + extra_dim),
        );
    }
    // Ink on paper lightens toward steel where the light runs across it.
    let alpha = ink * band * 0.22 * strength + ink * extra_light;
    return encode_out(u.color.rgb, min(alpha, 0.9));
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
            mode: 12.0,
            window: [13.0, 14.0],
            rect: [15.0, 16.0, 17.0, 18.0],
            color: [19.0, 20.0, 21.0, 22.0],
            flags: [23.0, 24.0, 25.0, 26.0],
            scale: 27.0,
        };
        let bytes = slot.bytes();
        let word = |i: usize| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        let numbers: Vec<f32> = (0..11).map(word).collect();
        assert_eq!(
            numbers,
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0]
        );
        assert_eq!(
            (word(11), word(12), word(13), word(14)),
            (12.0, 13.0, 14.0, 27.0)
        );
        // The rect, colour and flags are where the room shader's `vec4f`s are aligned to read.
        assert_eq!(
            (word(16), word(17), word(18), word(19)),
            (15.0, 16.0, 17.0, 18.0)
        );
        assert_eq!((word(20), word(24), word(25)), (19.0, 23.0, 24.0));
    }

    fn facts(t: f64) -> FrameFacts {
        FrameFacts {
            t,
            reduced: false,
            pointer: None,
            window: [1320.0, 860.0],
            dark: true,
            day: Some(0.3),
        }
    }

    #[test]
    fn a_door_at_rest_does_not_move() {
        let mut door = Door::new(0.0, 0.0);
        for k in 0..60 {
            door.step(k as f64 / 60.0, false, 0.0, false, 0.0, false);
        }
        assert!(!door.moving());
    }

    #[test]
    fn the_light_follows_the_pointer_and_rests_above_the_middle_without_one() {
        let mut light = Light::new(facts(0.0));
        assert_eq!(light.at, [660.0, -860.0 * 0.22]);
        let pointer = FrameFacts {
            pointer: Some((500.0, 300.0)),
            ..facts(1.0 / 60.0)
        };
        light.step(pointer);
        assert_eq!(light.target, [500.0, 300.0]);
        assert!(light.moving());
        for k in 2..600 {
            light.step(FrameFacts {
                pointer: Some((500.0, 300.0)),
                ..facts(k as f64 / 60.0)
            });
        }
        assert!((light.at[0] - 500.0).abs() < 0.5 && (light.at[1] - 300.0).abs() < 0.5);
        assert!(!light.moving(), "it comes to rest under the pointer");
        // The pointer leaves: the light goes back above the middle of the window.
        light.step(facts(11.0));
        assert_eq!(light.target, Light::rest([1320.0, 860.0]));
        for k in 1..600 {
            light.step(facts(11.0 + k as f64 / 60.0));
        }
        assert!(!light.moving());
    }

    #[test]
    fn the_lights_colour_carries_the_day() {
        assert_eq!(day_light(None), (STEEL, 0.0));
        assert_eq!(day_light(Some(0.0)), (STEEL, 0.0));
        assert_eq!(day_light(Some(0.4)), (TEAL, 0.2));
        assert_eq!(day_light(Some(-0.4)), (RED, 0.2));
        // A large move is capped at a strength of one; the shader keeps it low.
        assert_eq!(day_light(Some(9.0)).1, 1.0);
        // A gain and a loss of the same size light the room as strongly.
        assert_eq!(day_light(Some(0.7)).1, day_light(Some(-0.7)).1);
    }

    #[test]
    fn a_door_is_lit_from_the_rooms_light() {
        // The sidebar's mark, 25 points across, under a light at the window's middle.
        let direction = light_direction([660.0, 100.0], [100.0, 100.0], 25.0);
        assert!((direction[0] - LIGHT_TILT).abs() < 1e-4, "{direction:?}");
        assert!(direction[1].abs() < 1e-4, "{direction:?}");
        // A light right on the door keeps the page's tilt, so the steel never floods white.
        let on = light_direction([100.0, 100.0], [100.0, 100.0], 25.0);
        assert!(on[0] < 0.0 && on[1] < 0.0, "{on:?}");
        let magnitude = (on[0] * on[0] + on[1] * on[1]).sqrt();
        assert!(magnitude >= LIGHT_TILT * 0.5 - 1e-4, "{on:?}");
        // Within two radii the direction is not saturated.
        let near = light_direction([140.0, 100.0], [100.0, 100.0], 25.0);
        assert!(
            (near[0] - LIGHT_TILT * 1.6 / LIGHT_REACH).abs() < 1e-4,
            "{near:?}"
        );
    }

    #[test]
    fn a_new_door_turns_its_tumblers_once_and_settles() {
        let mut door = Door::new(2.0, 0.0);
        assert!(door.moving(), "it starts from rest with two turns to go");
        for k in 1..=180 {
            door.step(k as f64 / 60.0, false, 2.0, false, 0.0, false);
        }
        assert!((door.spin - 2.0).abs() < 0.01, "it turned: {}", door.spin);
        assert!(!door.moving(), "and came to rest");
    }

    #[test]
    fn a_thinking_door_turns_slowly_and_settles_on_a_whole_turn() {
        let mut door = Door::new(0.0, 0.0);
        door.step(1.0 / 60.0, false, 0.0, true, 0.0, false);
        for k in 2..=600 {
            door.step(k as f64 / 60.0, false, 0.0, true, 0.0, false);
        }
        // Ten seconds of thinking is a little more than one slow turn, and it never rests.
        assert!(door.spin > TAU * 0.9, "it turned: {}", door.spin);
        assert!(door.moving(), "a thinking door keeps the window drawing");
        // The answer lands: the tumblers settle, on a whole turn, with the heavy spring.
        for k in 601..=900 {
            door.step(k as f64 / 60.0, false, 0.0, false, 0.0, false);
        }
        assert!(!door.moving(), "and came to rest");
        assert!((door.spin / TAU - (door.spin / TAU).round()).abs() < 1e-3);
    }

    #[test]
    fn reduced_motion_lands_the_door_at_once() {
        let mut door = Door::new(1.0, 0.0);
        door.step(1.0, true, 3.0, false, 0.0, false);
        assert_eq!(door.spin, 3.0);
        assert!(!door.moving());
        // A light under reduced motion is where it belongs at once.
        let mut light = Light::new(facts(0.0));
        light.step(FrameFacts {
            t: 1.0,
            reduced: true,
            pointer: Some((500.0, 300.0)),
            ..facts(1.0)
        });
        assert_eq!(light.at, [500.0, 300.0]);
        assert!(!light.moving());
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
