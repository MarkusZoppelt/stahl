//! Stahl Capital on iOS: the desktop app's state and views, drawn by a Rune `LayerHost` onto the
//! `CAMetalLayer` of the Swift shell's view. The shell owns the window, the display link and
//! UIKit input, and calls these functions on the main thread.

use std::{
    env,
    ffi::{CStr, c_char, c_void},
    path::{Path, PathBuf},
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use portfolio_rs::config::AppConfig;
use rune::{
    GesturePhase, Key, KeyEvent, Mods, TouchPhase, WheelEvent, color::srgb_encode, embed::LayerHost,
};
use stahl::{
    app,
    backend::Backend,
    demo,
    state::{PickKind, State},
    theme::Theme,
};
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

/// `stahl_ios_tick` wants a file picked: present a document picker for one.
pub const TICK_PICK_FILE: u32 = 4;
/// `stahl_ios_tick` wants a folder picked.
pub const TICK_PICK_FOLDER: u32 = 8;

/// The demo workspace, seeded into the app's Documents on first launch.
fn demo_workspace(documents: &Path) -> PathBuf {
    let dir = documents.join("Demo");
    if !dir.join("positions.json").exists() {
        demo::seed_workspace(&dir);
    }
    dir
}

/// Where portfolio_rs keeps its config, and a portfolio without a workspace its daily valuations:
/// the app's Library, rather than the `~/.config` and `~/.local/share` it would use, which aren't
/// where an iOS app keeps things or what its backups cover. Only these: a workspace keeps its own
/// history, as on the Mac, which `PORTFOLIO_RS_DATA_DIR` would override.
fn use_app_library() {
    let Some(home) = env::var_os("HOME") else {
        return;
    };
    let library = PathBuf::from(home).join("Library/Application Support");
    for variable in ["XDG_CONFIG_HOME", "XDG_DATA_HOME"] {
        if env::var_os(variable).is_none() {
            // SAFETY: called before the Tokio runtime or anything else here starts a thread.
            unsafe { env::set_var(variable, &library) };
        }
    }
}

/// `path` inside an app container that has since moved, as it is now: iOS may move an app's
/// container on an update, and a portfolio kept in Stahl's own Documents moves with it.
fn moved_with_container(path: &str, home: &str) -> Option<String> {
    const CONTAINERS: &str = "/Containers/Data/Application/";
    let (_, rest) = path.split_once(CONTAINERS)?;
    let (_, inside) = rest.split_once('/')?;
    let moved = format!("{}/{inside}", home.trim_end_matches('/'));
    (moved != path && !Path::new(path).exists() && Path::new(&moved).exists()).then_some(moved)
}

/// Point the remembered portfolio at where its container is now.
fn follow_container() {
    let (Some(home), Ok(mut config)) = (env::var("HOME").ok(), AppConfig::load()) else {
        return;
    };
    let mut changed = false;
    for path in [&mut config.workspace_dir, &mut config.portfolio_file] {
        if let Some(moved) = path
            .as_deref()
            .and_then(|path| moved_with_container(path, &home))
        {
            *path = Some(moved);
            changed = true;
        }
    }
    if changed && let Err(error) = config.save() {
        eprintln!("stahl: could not update the remembered portfolio: {error:#}");
    }
}

/// Starts Stahl on `layer`, a `CAMetalLayer *` that outlives the host. `documents` is the app's
/// Documents directory. With `STAHL_IOS_DEMO=1` in the environment it opens a made-up portfolio
/// on fixture prices; otherwise the portfolio last opened, with live prices. Returns null if the
/// GPU surface can't be made.
///
/// # Safety
/// Call on the main thread, before any other function here. `layer` must be a valid
/// `CAMetalLayer`, and `documents` a valid NUL-terminated UTF-8 path.
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
    let demo = env::var_os("STAHL_IOS_DEMO").is_some_and(|value| value == "1");
    use_app_library();
    let tokio = Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to start the Tokio runtime");
    let backend = if demo {
        let workspace = demo_workspace(&documents);
        demo::backend(&documents, Some(&workspace), tokio.handle().clone())
    } else {
        follow_container();
        Backend::load(tokio.handle().clone())
    };
    let mut state = State::new(backend, None);
    // Rune's dialogs can't show in an embedded view; the shell shows UIKit's document picker.
    state.platform_picker = true;

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
    match h.host.runtime_mut().state.take_pick() {
        Some((PickKind::File, _)) => flags |= TICK_PICK_FILE,
        Some((PickKind::Folder, _)) => flags |= TICK_PICK_FOLDER,
        None => {}
    }
    flags
}

/// The document picker answered: a path the shell has security-scoped access to, or null when it
/// was cancelled.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`], and `path` be null or NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_picked(host_ptr: *mut Host, path: *const c_char) {
    let path = (!path.is_null()).then(|| {
        unsafe { CStr::from_ptr(path) }
            .to_string_lossy()
            .into_owned()
    });
    let h = host(host_ptr);
    h.host
        .runtime_mut()
        .update(move |state, _| state.picked(path));
    h.dirty.store(true, Ordering::Release);
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_portfolio_follows_its_moved_container_and_nothing_else_does() {
        let root = env::temp_dir().join(format!("stahl-container-{}", std::process::id()));
        let new = root.join("Containers/Data/Application/NEW");
        fs::create_dir_all(new.join("Documents/Mine")).unwrap();
        let home = new.to_string_lossy().into_owned();
        let old = root.join("Containers/Data/Application/OLD/Documents/Mine");
        let old = old.to_string_lossy();

        assert_eq!(
            moved_with_container(&old, &home),
            Some(format!("{home}/Documents/Mine"))
        );
        assert_eq!(
            moved_with_container(&format!("{home}/Documents/Mine"), &home),
            None
        );
        assert_eq!(
            moved_with_container("/private/var/mobile/Library/Mobile Documents/x", &home),
            None
        );
        let gone = old.replace("Mine", "Gone");
        assert_eq!(
            moved_with_container(&gone, &home),
            None,
            "nothing there to follow"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
