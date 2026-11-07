//! Stahl Capital on iOS: the desktop app's state and views, drawn by a Rune `LayerHost` onto the
//! `CAMetalLayer` of the Swift shell's view. The shell owns the window, the display link and
//! UIKit input, and calls these functions on the main thread.

use std::{
    ffi::{CStr, c_char, c_void},
    path::{Path, PathBuf},
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use rune::{
    GesturePhase, Key, KeyEvent, Mods, TouchPhase, WheelEvent, color::srgb_encode, embed::LayerHost,
};
use stahl::{app, demo, state::State, theme::Theme};
use tokio::runtime::{Builder, Runtime};

/// `stahl_ios_tick` presented a frame.
pub const TICK_DREW: u32 = 1;
/// A text field has focus: the shell should show the keyboard.
pub const TICK_WANTS_KEYBOARD: u32 = 2;

pub struct Host {
    host: LayerHost<State>,
    /// Set by the backend's waker and by input: the next tick has work to do.
    dirty: Arc<AtomicBool>,
    animating: bool,
    /// Last, so the runtime is dropped after the state whose work runs on it.
    _tokio: Runtime,
}

/// The demo workspace, seeded into the app's Documents on first launch.
fn demo_workspace(documents: &Path) -> PathBuf {
    let dir = documents.join("Demo");
    if !dir.join("positions.json").exists() {
        demo::seed_workspace(&dir);
    }
    dir
}

/// Starts Stahl on `layer`, a `CAMetalLayer *` that outlives the host. `documents` is the app's
/// Documents directory. Returns null if the GPU surface can't be made.
///
/// # Safety
/// Call on the main thread. `layer` must be a valid `CAMetalLayer`, and `documents` a valid
/// NUL-terminated UTF-8 path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_new(
    layer: *mut c_void,
    documents: *const c_char,
    width: f32,
    height: f32,
    scale: f32,
    dark: bool,
) -> *mut Host {
    let documents = PathBuf::from(
        unsafe { CStr::from_ptr(documents) }
            .to_string_lossy()
            .as_ref(),
    );
    let tokio = Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to start the Tokio runtime");
    let workspace = demo_workspace(&documents);
    let backend = demo::backend(&documents, Some(&workspace), tokio.handle().clone());
    let state = State::new(backend, None);

    let dirty = Arc::new(AtomicBool::new(true));
    let wake = {
        let dirty = dirty.clone();
        Arc::new(move || dirty.store(true, Ordering::Release))
    };
    // SAFETY: the caller hands a live CAMetalLayer and calls on the main thread.
    let host = unsafe {
        LayerHost::from_core_animation_layer(
            layer,
            &app::spec(),
            state,
            (width, height),
            scale,
            dark,
            wake,
        )
    };
    match host {
        Ok(host) => Box::into_raw(Box::new(Host {
            host,
            dirty,
            animating: true,
            _tokio: tokio,
        })),
        Err(error) => {
            eprintln!("stahl: could not start: {error}");
            ptr::null_mut()
        }
    }
}

/// # Safety
/// `host` must come from [`stahl_ios_new`] and not be used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_free(host: *mut Host) {
    if !host.is_null() {
        drop(unsafe { Box::from_raw(host) });
    }
}

fn host<'a>(host: *mut Host) -> &'a mut Host {
    // SAFETY: every entry point's contract: a live host from `stahl_ios_new`, on the main thread.
    unsafe { &mut *host }
}

/// One display-link tick: draws if anything changed or is moving. Returns `TICK_*` flags.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_tick(host_ptr: *mut Host) -> u32 {
    let h = host(host_ptr);
    let mut flags = 0;
    if h.dirty.swap(false, Ordering::AcqRel) || h.animating {
        let frame = h.host.frame();
        h.animating = frame.animating;
        if frame.drew {
            flags |= TICK_DREW;
        }
    }
    if h.host.runtime().ime_area.is_some() && !h.host.runtime().has_secure_focus() {
        flags |= TICK_WANTS_KEYBOARD;
    }
    flags
}

/// The view's size in points changed, or its scale. Zero pauses drawing.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_resize(
    host_ptr: *mut Host,
    width: f32,
    height: f32,
    scale: f32,
) {
    let h = host(host_ptr);
    h.host.resize((width, height), scale);
    h.dirty.store(true, Ordering::Release);
}

/// Whether to lay out for a phone: UIKit's horizontal size class is compact.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_set_compact(host_ptr: *mut Host, compact: bool) {
    let h = host(host_ptr);
    if h.host.runtime().state.compact != compact {
        h.host.runtime_mut().state.compact = compact;
        h.dirty.store(true, Ordering::Release);
    }
}

/// The app's background as `0xRRGGBB` in sRGB, for the shell to paint behind the safe area.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_background(host_ptr: *mut Host) -> u32 {
    let state = &host(host_ptr).host.runtime().state;
    let background = Theme::new(state.is_dark()).kit.c.bg_base;
    let channel = |linear: f32| (srgb_encode(linear).clamp(0.0, 1.0) * 255.0).round() as u32;
    channel(background.r) << 16 | channel(background.g) << 8 | channel(background.b)
}

/// A touch in view points. `phase`: 0 began, 1 moved, 2 ended, 3 cancelled. `time` in seconds.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_touch(
    host_ptr: *mut Host,
    id: u64,
    phase: u32,
    x: f32,
    y: f32,
    time: f64,
) {
    let phase = match phase {
        0 => TouchPhase::Started,
        1 => TouchPhase::Moved,
        2 => TouchPhase::Ended,
        _ => TouchPhase::Cancelled,
    };
    let h = host(host_ptr);
    h.host.runtime_mut().touch(id, phase, x, y, time);
    h.dirty.store(true, Ordering::Release);
}

fn key(h: &mut Host, key: Key) {
    h.host.runtime_mut().key(KeyEvent {
        key,
        mods: Mods::default(),
    });
    h.dirty.store(true, Ordering::Release);
}

/// The keys a hardware keyboard sends that aren't text, numbered as in `stahl.h`.
const KEYS: [Key; 13] = [
    Key::Escape,
    Key::Tab,
    Key::Enter,
    Key::Backspace,
    Key::Delete,
    Key::Left,
    Key::Right,
    Key::Up,
    Key::Down,
    Key::Home,
    Key::End,
    Key::PageUp,
    Key::PageDown,
];

/// A key from a hardware keyboard. `key` is 0 for a chord with ⌘ and `character` (its key, without
/// modifiers), or 1 + an index into [`KEYS`]. `mods`: 1 Shift, 2 ⌘, 4 Option, 8 Control.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_key(host_ptr: *mut Host, key: u32, character: u32, mods: u32) {
    let pressed = match key {
        0 => match char::from_u32(character) {
            Some(character) => Key::Command(character.to_ascii_lowercase()),
            None => return,
        },
        index => match KEYS.get(index as usize - 1) {
            Some(key) => key.clone(),
            None => return,
        },
    };
    let h = host(host_ptr);
    h.host.runtime_mut().key(KeyEvent {
        key: pressed,
        mods: Mods {
            shift: mods & 1 != 0,
            cmd: mods & 2 != 0,
            logo: mods & 2 != 0,
            alt: mods & 4 != 0,
            ctrl: mods & 8 != 0,
        },
    });
    h.dirty.store(true, Ordering::Release);
}

/// A trackpad or mouse pointer hovering at `x`, `y` in view points, or gone when `inside` is false.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_hover(host_ptr: *mut Host, x: f32, y: f32, inside: bool) {
    let h = host(host_ptr);
    if inside {
        h.host.runtime_mut().pointer_move(x, y);
    } else {
        h.host.runtime_mut().pointer_leave();
    }
    h.dirty.store(true, Ordering::Release);
}

/// Two-finger trackpad or mouse-wheel scrolling at `x`, `y`: `dx`, `dy` in points, the way the
/// fingers moved. `phase`: 0 began, 1 moved, 2 ended.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_scroll(
    host_ptr: *mut Host,
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    phase: u32,
) {
    let phase = match phase {
        0 => GesturePhase::Began,
        1 => GesturePhase::Moved,
        _ => GesturePhase::Ended,
    };
    let h = host(host_ptr);
    h.host
        .runtime_mut()
        .wheel_event(WheelEvent::new(dx, dy).at(x, y).phase(phase).precise(true));
    h.dirty.store(true, Ordering::Release);
}

/// Text from the software keyboard (`UIKeyInput.insertText`).
///
/// # Safety
/// `host` must come from [`stahl_ios_new`], and `text` be NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_insert_text(host_ptr: *mut Host, text: *const c_char) {
    let text = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    let h = host(host_ptr);
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            key(h, Key::Enter);
        }
        if !line.is_empty() {
            key(h, Key::Text(line.to_string()));
        }
    }
}

/// `UIKeyInput.deleteBackward`.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_delete_backward(host_ptr: *mut Host) {
    key(host(host_ptr), Key::Backspace);
}
