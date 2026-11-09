//! Stahl Capital on iOS: the desktop app's state and views, drawn by a Rune `LayerHost` onto the
//! `CAMetalLayer` of the Swift shell's view. The shell owns the window, the display link and
//! UIKit input, and calls these functions on the main thread.

use std::{
    env,
    ffi::{CStr, CString, c_char, c_void},
    mem,
    path::{Path, PathBuf},
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use portfolio_rs::config::AppConfig;
use rune::{
    GesturePhase, Key, KeyEvent, Mods, TouchPhase, WheelEvent,
    clipboard::{self, MemoryClipboard},
    color::srgb_encode,
    embed::LayerHost,
};
use stahl::{
    app,
    backend::Backend,
    demo,
    state::{Page, PickKind, State},
    theme::Theme,
};
use tokio::runtime::{Builder, Runtime};

// What `stahl_ios_tick` tells the shell, as bits. The same numbers are in `stahl.h`.
/// A frame was presented.
pub const TICK_DREW: u32 = 1;
/// A text field has focus: the shell should show the keyboard.
pub const TICK_WANTS_KEYBOARD: u32 = 2;
/// The app wants a file picked: present a document picker for one.
pub const TICK_PICK_FILE: u32 = 4;
/// The app wants a folder picked.
pub const TICK_PICK_FOLDER: u32 = 8;
/// The field with focus is a secret: the keyboard should hide what's typed.
pub const TICK_SECURE: u32 = 16;
/// The field with focus takes a number: a keyboard of digits.
pub const TICK_NUMBER: u32 = 32;
/// The app copied something: `stahl_ios_take_copied` has it for the system pasteboard.
pub const TICK_COPIED: u32 = 64;
/// Somewhere else now: a page, a tab, a holding. A selection haptic.
pub const TICK_HAPTIC_SELECT: u32 = 128;
/// Something worked: a toast came up. A success haptic.
pub const TICK_HAPTIC_SUCCESS: u32 = 256;
/// Something failed: an error came up. A warning haptic.
pub const TICK_HAPTIC_WARNING: u32 = 512;
/// The vault's door turned. A heavy, rigid knock.
pub const TICK_HAPTIC_TURN: u32 = 1024;
/// A finger crossed a chart's day, a donut's slice, or the point where a pull refreshes. A light tick.
pub const TICK_HAPTIC_TICK: u32 = 2048;
/// The lock setting changed: `stahl_ios_lock_enabled` says what to remember.
pub const TICK_LOCK_CHANGED: u32 = 4096;

pub struct Host {
    host: LayerHost<State>,
    /// Set by the backend's waker and by input: the next tick has work to do.
    dirty: Arc<AtomicBool>,
    animating: bool,
    /// iOS has no clipboard Rune can reach: the app copies here, and the shell moves it on.
    clipboard: MemoryClipboard,
    copied: Option<String>,
    seen: Seen,
    /// Haptics an input asked for, played on the next tick.
    pending: u32,
    /// Last, so the runtime is dropped after the state whose work runs on it.
    _tokio: Runtime,
}

/// What the haptics follow, as of the last tick.
#[derive(Default)]
struct Seen {
    place: (Option<Page>, bool, Option<usize>),
    toast: Option<Instant>,
    error: Option<String>,
    door_turns: u32,
    point: (Option<usize>, Option<usize>),
    pull_past: bool,
    lock: Option<bool>,
}

impl Seen {
    fn of(state: &State) -> Seen {
        Seen {
            place: (Some(state.page), state.more, state.selected),
            toast: state.toast.as_ref().map(|(_, at)| *at),
            error: state.error.clone(),
            door_turns: state.door_turns,
            point: (state.chart_point, state.donut_hover),
            pull_past: state.pull.past(),
            lock: state.lock,
        }
    }

    /// The haptics for going from `self` to `now`.
    fn haptics(&self, now: &Seen) -> u32 {
        let mut flags = 0;
        if self.place.0.is_some() && self.place != now.place {
            flags |= TICK_HAPTIC_SELECT;
        }
        if now.toast.is_some() && now.toast != self.toast {
            flags |= TICK_HAPTIC_SUCCESS;
        }
        if now.error.is_some() && now.error != self.error {
            flags |= TICK_HAPTIC_WARNING;
        }
        if now.door_turns != self.door_turns {
            flags |= TICK_HAPTIC_TURN;
        }
        let pointed =
            |point: Option<usize>, before: Option<usize>| point.is_some() && point != before;
        if pointed(now.point.0, self.point.0)
            || pointed(now.point.1, self.point.1)
            || (now.pull_past && !self.pull_past)
        {
            flags |= TICK_HAPTIC_TICK;
        }
        if self.place.0.is_some() && now.lock != self.lock {
            flags |= TICK_LOCK_CHANGED;
        }
        flags
    }
}

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
        Ok(mut host) => {
            let clipboard = clipboard::memory();
            host.runtime_mut().use_clipboard(&clipboard);
            Box::into_raw(Box::new(Host {
                host,
                dirty,
                animating: true,
                clipboard,
                copied: None,
                seen: Seen::default(),
                pending: 0,
                _tokio: tokio,
            }))
        }
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
    let rt = h.host.runtime();
    // Rune leaves a secret field out of the IME, but it still needs a keyboard.
    if rt.has_secure_focus() {
        flags |= TICK_WANTS_KEYBOARD | TICK_SECURE;
    } else if rt.ime_area.is_some() {
        flags |= TICK_WANTS_KEYBOARD;
        if rt.state.number_focused() {
            flags |= TICK_NUMBER;
        }
    }
    let seen = Seen::of(&rt.state);
    flags |= h.seen.haptics(&seen) | mem::take(&mut h.pending);
    h.seen = seen;
    // The pull's door spins until the prices it asked for are in.
    if h.host.runtime().state.pull_refreshing && !h.host.runtime().state.loading {
        h.host
            .runtime_mut()
            .update(|state, _| state.pull_refreshing = false);
    }
    let copied = h.clipboard.text().map(|text| text.to_string());
    if copied.is_some() && copied != h.copied {
        flags |= TICK_COPIED;
    }
    h.copied = copied;
    match h.host.runtime_mut().state.take_pick() {
        Some((PickKind::File, _)) => flags |= TICK_PICK_FILE,
        Some((PickKind::Folder, _)) => flags |= TICK_PICK_FOLDER,
        None => {}
    }
    flags
}

/// What the app last copied, for the system pasteboard: a string to free with
/// [`stahl_ios_free_string`], or null.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_take_copied(host_ptr: *mut Host) -> *mut c_char {
    host(host_ptr)
        .copied
        .as_deref()
        .and_then(|text| CString::new(text).ok())
        .map_or(ptr::null_mut(), CString::into_raw)
}

/// # Safety
/// `text` must come from this library, and not be used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_free_string(text: *mut c_char) {
    if !text.is_null() {
        drop(unsafe { CString::from_raw(text) });
    }
}

/// Text pasted from the system pasteboard, into the field with focus.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`], and `text` be NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_paste(host_ptr: *mut Host, text: *const c_char) {
    let text = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    let h = host(host_ptr);
    h.host.runtime_mut().paste(text);
    h.dirty.store(true, Ordering::Release);
}

/// The app is going to the background: save unsaved edits, since iOS may end it there without
/// asking. Returns whether a save started.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_backgrounded(host_ptr: *mut Host) -> bool {
    let h = host(host_ptr);
    let mut saving = false;
    h.host.runtime_mut().update(|state, _| {
        if state.dirty {
            state.save();
            saving = true;
        }
    });
    saving
}

/// The app is back: prices that went stale while it was away refresh.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_foregrounded(host_ptr: *mut Host) {
    let h = host(host_ptr);
    h.host
        .runtime_mut()
        .update(|state, _| state.refresh_if_stale());
    h.dirty.store(true, Ordering::Release);
}

/// A software keyboard came up over the app, or went away.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_set_keyboard(host_ptr: *mut Host, up: bool) {
    let h = host(host_ptr);
    if h.host.runtime().state.keyboard != up {
        h.host.runtime_mut().update(|state, _| state.keyboard = up);
        h.dirty.store(true, Ordering::Release);
    }
}

/// The phone was shaken: the amounts hide, or show again.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_shake(host_ptr: *mut Host) {
    let h = host(host_ptr);
    h.host.runtime_mut().update(|state, _| state.shaken());
    h.dirty.store(true, Ordering::Release);
}

/// The system's Reduce Motion setting.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_set_reduced_motion(host_ptr: *mut Host, reduced: bool) {
    let h = host(host_ptr);
    h.host.runtime_mut().set_system_reduced_motion(reduced);
    h.dirty.store(true, Ordering::Release);
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
        h.host
            .runtime_mut()
            .update(|state, _| state.compact = compact);
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
    let rt = h.host.runtime_mut();
    rt.touch(id, phase, x, y, time);
    // A finger on a chart reads it out as a pointer hovering there would, until it lifts.
    match phase {
        TouchPhase::Started | TouchPhase::Moved => rt.pointer_move(x, y),
        TouchPhase::Ended | TouchPhase::Cancelled => rt.pointer_leave(),
    }
    // Letting go: a page pulled far enough refreshes, and slid rows come to rest.
    if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
        let mut refreshing = false;
        rt.update(|state, tx| {
            refreshing = phase == TouchPhase::Ended && state.pull_released();
            state.settle_swipes(tx);
        });
        if refreshing {
            h.pending |= TICK_HAPTIC_TURN;
        }
    }
    h.dirty.store(true, Ordering::Release);
}

/// The system's back gesture: a swipe in from the left edge. Returns whether it went back.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_back(host_ptr: *mut Host) -> bool {
    let h = host(host_ptr);
    let went = h.host.runtime_mut().update_if(|state, _| state.go_back());
    h.dirty.store(true, Ordering::Release);
    went
}

/// Whether this device can lock the app, what it calls its lock, and whether the person asked
/// for it.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`], and `name` be NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_set_lock(
    host_ptr: *mut Host,
    available: bool,
    enabled: bool,
    name: *const c_char,
) {
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    let h = host(host_ptr);
    h.host.runtime_mut().update(|state, _| {
        state.lock = available.then_some(enabled);
        state.lock_name = name;
    });
    h.dirty.store(true, Ordering::Release);
}

/// Whether opening the app asks for Face ID, as the person last set it.
///
/// # Safety
/// `host` must come from [`stahl_ios_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stahl_ios_lock_enabled(host_ptr: *mut Host) -> bool {
    host(host_ptr).host.runtime().state.lock == Some(true)
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
