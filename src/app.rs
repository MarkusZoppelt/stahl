//! The application: window chrome, native menus, and how it starts.

use std::{thread, time::Duration};

use rune::{App, AppHandle, AppSpec, Chrome, Cx, El, Env, MenuItem, Runtime, Tx};

use crate::{
    backend::Backend,
    state::{Page, State},
    theme::FONTS,
    update::Updater,
    view,
};

/// A named state to render, for `examples/demo.rs`.
pub type Scene = (&'static str, fn(&mut State));

/// `init` must be a plain fn, so the real first state comes through `App::init`.
fn placeholder() -> State {
    unreachable!("App::init provides the state")
}

fn view_state(s: &State, cx: &mut Cx<State>) -> El {
    view::view(s, cx)
}

pub fn spec() -> AppSpec<State> {
    AppSpec {
        min_size: Some((960.0, 640.0)),
        setup: Some(|s, waker| {
            s.set_waker(waker);
            s.startup();
        }),
        poll: Some(State::poll),
        on_key: Some(view::on_key),
        fonts: FONTS,
        ..AppSpec::new("Stahl Capital", (1320.0, 860.0), placeholder, view_state)
    }
}

fn item(
    title: &str,
    menu: &str,
    keys: Option<&str>,
    action: fn(&mut State, &mut Tx),
) -> MenuItem<State> {
    let mut item = MenuItem::new(title, action).menu(menu).keys_first();
    if let Some(keys) = keys {
        item = item.key_equivalent(keys.parse().expect("menu shortcuts are valid"));
    }
    item
}

fn page_item(page: Page, action: fn(&mut State, &mut Tx)) -> MenuItem<State> {
    let keys = page.shortcut().map(|key| format!("cmd+{key}"));
    item(page.label(), "Go", keys.as_deref(), action)
}

/// Rune only has native menus on macOS, and elsewhere logs an error for trying. Everything here is
/// also a shortcut or a command palette entry, so other platforms lose nothing.
fn with_menus(app: App<State>) -> App<State> {
    if !cfg!(target_os = "macos") {
        return app;
    }
    app.menu_item(item(
        "Open Portfolio File…",
        "File",
        Some("cmd+o"),
        |s, tx| s.ask_for_file(tx),
    ))
    .menu_item(item(
        "Open Workspace…",
        "File",
        Some("cmd+shift+o"),
        |s, tx| s.ask_for_workspace(tx),
    ))
    .menu_item(item("Close", "File", Some("cmd+shift+w"), |s, _| s.reset()))
    .menu_item(MenuItem::separator().menu("File"))
    .menu_item(item("Add Position…", "File", Some("cmd+n"), |s, _| {
        s.open_form(None)
    }))
    .menu_item(item("Save", "File", Some("cmd+s"), |s, _| s.save()))
    .menu_item(item("Command Palette…", "View", Some("cmd+k"), |s, _| {
        s.open_palette()
    }))
    .menu_item(MenuItem::separator().menu("View"))
    .menu_item(item("Refresh Prices", "View", Some("cmd+r"), |s, _| {
        s.refresh_prices()
    }))
    .menu_item(item(
        "Hide or Show Amounts",
        "View",
        Some("cmd+shift+h"),
        |s, _| s.privacy = !s.privacy,
    ))
    .menu_item(item("Toggle Light/Dark Theme", "View", None, |s, _| {
        s.toggle_theme()
    }))
    .menu_item(item(
        "Toggle Sidebar",
        "View",
        Some("cmd+shift+b"),
        |s, _| s.sidebar.toggle(),
    ))
    .menu_item(page_item(Page::Overview, |s, _| s.navigate(Page::Overview)))
    .menu_item(page_item(Page::Holdings, |s, _| s.navigate(Page::Holdings)))
    .menu_item(page_item(Page::Allocation, |s, _| {
        s.navigate(Page::Allocation)
    }))
    .menu_item(page_item(Page::Performance, |s, _| {
        s.navigate(Page::Performance)
    }))
    .menu_item(MenuItem::separator().menu("Go"))
    .menu_item(page_item(Page::Review, |s, _| s.navigate(Page::Review)))
    .menu_item(page_item(Page::Rebalance, |s, _| {
        s.navigate(Page::Rebalance)
    }))
    .menu_item(page_item(Page::Policy, |s, _| s.navigate(Page::Policy)))
    .menu_item(page_item(Page::Documents, |s, _| {
        s.navigate(Page::Documents)
    }))
    .menu_item(MenuItem::separator().menu("Go"))
    .menu_item(page_item(Page::Briefing, |s, _| s.navigate(Page::Briefing)))
    .menu_item(page_item(Page::Health, |s, _| s.navigate(Page::Health)))
    .menu_item(item("Stahl Guide", "Help", Some("cmd+/"), |s, _| {
        s.toggle_guide()
    }))
    .menu_item(
        MenuItem::new("Refresh Prices", |s: &mut State, _| s.refresh_prices())
            .id("dock-refresh")
            .dock(),
    )
    // With no named menu, this lands in the application menu, where Settings belongs.
    .menu_item(
        MenuItem::new("Settings…", |s: &mut State, _| s.navigate(Page::Settings))
            .key_equivalent("cmd+,".parse().expect("menu shortcuts are valid"))
            .keys_first(),
    )
    .menu_item(MenuItem::new(
        "Check for Updates…",
        |s: &mut State, _: &mut Tx| s.check_for_updates(false),
    ))
}

fn configured(spec: AppSpec<State>, init: impl FnOnce() -> State + 'static) -> App<State> {
    App::new(spec)
        .init(init)
        .chrome(Chrome {
            full_size_content: true,
            traffic_lights: Some((16.0, 17.0)),
            drag_regions: true,
        })
        .standard_menus()
        // Closing with unsaved changes brings the window back to ask.
        .keep_running()
        .on_close(State::on_window_close)
        .on_focus(|s, focused, _| {
            if focused {
                s.refresh_if_stale();
            }
        })
}

/// What to tell someone whose window couldn't open, given Rune's reason.
fn window_failure(reason: &str) -> String {
    let hint = if cfg!(target_os = "linux") {
        "Stahl draws with the GPU, so this usually means a missing driver. Install a Vulkan \
         driver, such as mesa-vulkan-drivers on Debian and Ubuntu, which also works with no GPU."
    } else {
        "Stahl draws with the GPU, so this usually means a missing or outdated graphics driver."
    };
    format!("could not open the window: {reason}\n{hint}")
}

/// Asks the app to quit if its first window can't be created. Rune would otherwise keep running
/// with no window, waiting for an open that nothing will ever ask for.
fn quit_without_a_window(handle: AppHandle<State>) {
    thread::spawn(move || {
        while !handle.is_visible() {
            if handle.window_error().is_some() {
                handle.quit();
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    });
}

/// Runs until the app quits, or says why it couldn't start.
fn launch(app: App<State>) -> Result<(), String> {
    let handle = app.handle();
    quit_without_a_window(handle.clone());
    app.try_run()?;
    handle
        .window_error()
        .map_or(Ok(()), |reason| Err(window_failure(&reason)))
}

pub fn run(
    backend: Backend,
    initial_file: Option<String>,
    rune_args: Vec<String>,
) -> Result<(), String> {
    let app = with_menus(configured(spec(), move || {
        State::new(backend, initial_file).with_updater(Updater::github())
    }));
    let handle = app.handle();
    let app = app.on_ready({
        let handle = handle.clone();
        move |_| {
            let reopen = handle.clone();
            handle.update(move |s, _| s.reopen = Some(Box::new(move || reopen.open())));
        }
    });
    launch(app.args(rune_args))
}

/// [`run`] with named states to render, for `examples/demo.rs`.
pub fn run_with_scenes(
    backend: Backend,
    rune_args: Vec<String>,
    scenes: &'static [Scene],
) -> Result<(), String> {
    let spec = AppSpec { scenes, ..spec() };
    launch(with_menus(configured(spec, move || State::new(backend, None))).args(rune_args))
}

/// The app without a window, for tests and tooling: every scene and handler as in the real one.
pub fn headless(
    backend: Backend,
    initial_file: Option<String>,
    size: (f32, f32),
) -> Runtime<State> {
    headless_with_updater(backend, initial_file, size, Updater::disabled())
}

/// [`headless`] with an updater, which tests point at a fake release source and temp files.
pub fn headless_with_updater(
    backend: Backend,
    initial_file: Option<String>,
    size: (f32, f32),
    updater: Updater,
) -> Runtime<State> {
    let mut runtime = configured(spec(), move || {
        State::new(backend, initial_file).with_updater(updater)
    })
    .headless_runtime(size, Env::new(true, 2.0));
    runtime.state.startup();
    runtime.state.settle();
    runtime
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_failure_says_why_and_what_to_do() {
        let message = window_failure("Failed to create surface for any enabled backend: {}");
        assert!(message.contains("Failed to create surface"));
        assert!(message.contains("driver"));
        if cfg!(target_os = "linux") {
            assert!(message.contains("mesa-vulkan-drivers"));
        }
    }
}
