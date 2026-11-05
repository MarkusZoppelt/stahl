//! The application: window, menus, shortcuts, and the sidebar shell.

use gpui_kit::{
    Action, AnyView, AnyWindowHandle, App, AppContext as _, Bounds, ClipboardItem, Context, Div,
    Entity, ExternalPaths, FontWeight, Global, IntoElement, KeyBinding, Menu, MenuItem, OsAction,
    ParentElement as _, PromptLevel, Render, Stateful, StatefulInteractiveElement as _,
    Styled as _, SystemMenuType, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowOptions,
    actions, application,
    assets::IconName,
    base::TestSupportExt as _,
    component::{
        Sizable as _, WindowExt as _,
        button::{Button, ButtonVariants as _},
        command::{Command, CommandGroup, CommandItem, CommandState},
        input::{Copy, Cut, Paste, Redo, SelectAll, Undo},
        scroll::ScrollableElement as _,
        spinner::Spinner,
    },
    div, img, init as init_kit, open_window, point,
    prelude::{FluentBuilder as _, InteractiveElement as _},
    px, size,
};
use portfolio_rs::config::ThemeMode;

use crate::{
    assets::{STAHL_MARK, StahlAssets},
    backend::Backend,
    model::{AppModel, Privacy},
    pages::{self, View},
    theme::{self, ActivePalette as _, tint},
    ui,
};

actions!(
    stahl,
    [
        Quit,
        Hide,
        HideOthers,
        ShowAll,
        OpenFile,
        OpenWorkspace,
        Save,
        RefreshPrices,
        ToggleTheme,
        TogglePrivacy,
        CommandPalette,
        Minimize,
        Zoom,
        GoOverview,
        GoHoldings,
        GoAllocation,
        GoPerformance,
        GoReview,
        GoRebalance,
        GoPolicy,
        GoDocuments,
        GoBriefing,
        GoHealth,
        GoSettings,
    ]
);

/// Room for the macOS traffic lights over the sidebar.
const TITLEBAR_INSET: f32 = if cfg!(target_os = "macos") { 40. } else { 12. };

/// How app-wide actions reach the window, whatever has focus.
struct Main {
    window: AnyWindowHandle,
    shell: WeakEntity<Shell>,
}

impl Global for Main {}

/// Reach the shell from an action. Deferred, because key and menu dispatch is
/// already updating the window, which cannot be re-entered.
fn with_shell(
    cx: &mut App,
    f: impl FnOnce(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
) {
    let Some(main) = cx.try_global::<Main>() else {
        return;
    };
    let (window, shell) = (main.window, main.shell.clone());
    cx.defer(move |cx| {
        let _ = window.update(cx, move |_, window, cx| {
            let _ = shell.update(cx, |shell, cx| f(shell, window, cx));
        });
    });
}

fn on_navigate<A: Action>(view: View, cx: &mut App) {
    cx.on_action(move |_: &A, cx| {
        with_shell(cx, move |shell, window, cx| {
            shell.navigate(view, window, cx);
        });
    });
}

pub fn run(backend: Backend, initial_file: Option<String>) {
    application().with_assets(StahlAssets).run(move |cx| {
        install(backend, cx);
        let bounds = Bounds::centered(None, size(px(1320.), px(860.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(960.), px(640.))),
            titlebar: Some(TitlebarOptions {
                title: Some("Stahl Capital".into()),
                // The sidebar runs up behind the traffic lights on macOS.
                appears_transparent: cfg!(target_os = "macos"),
                traffic_light_position: Some(point(px(14.), px(14.))),
            }),
            app_id: Some("net.zoppelt.stahl".to_string()),
            ..Default::default()
        };
        open_main_window(options, initial_file, cx);
        cx.activate(true);
    });
}

/// Set up everything the window relies on.
pub fn install(backend: Backend, cx: &mut App) {
    init_kit(cx);
    theme::init(ThemeMode::Dark, cx);
    cx.set_global(backend);
    cx.set_global(Privacy::default());
    bind_keys(cx);
    register_actions(cx);
    set_menus(cx);
}

pub fn open_main_window(
    options: WindowOptions,
    initial_file: Option<String>,
    cx: &mut App,
) -> (AnyWindowHandle, Entity<Shell>) {
    let (window, shell) = open_window(options, cx, |window, cx| {
        cx.new(|cx| Shell::new(initial_file, window, cx))
    })
    .expect("failed to open the main window");
    cx.set_global(Main {
        window,
        shell: shell.downgrade(),
    });
    (window, shell)
}

fn register_actions(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| {
        with_shell(cx, |shell, window, cx| shell.close_or_ask(window, cx));
    });
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &OpenFile, cx| {
        with_shell(cx, |shell, _, cx| {
            shell.model.update(cx, |model, cx| model.open_file(cx));
        });
    });
    cx.on_action(|_: &OpenWorkspace, cx| {
        with_shell(cx, |shell, _, cx| {
            shell.model.update(cx, |model, cx| model.open_workspace(cx));
        });
    });
    cx.on_action(|_: &Save, cx| {
        with_shell(cx, |shell, _, cx| {
            shell.model.update(cx, |model, cx| model.save(cx));
        });
    });
    cx.on_action(|_: &RefreshPrices, cx| {
        with_shell(cx, |shell, _, cx| {
            shell.model.update(cx, |model, cx| model.refresh_prices(cx));
        });
    });
    cx.on_action(|_: &ToggleTheme, cx| with_shell(cx, |shell, _, cx| shell.toggle_theme(cx)));
    cx.on_action(|_: &TogglePrivacy, cx| toggle_privacy(cx));
    cx.on_action(|_: &CommandPalette, cx| {
        with_shell(cx, |shell, window, cx| {
            let is_workspace = shell.model.read(cx).is_workspace();
            open_command_palette(is_workspace, window, cx);
        });
    });
    cx.on_action(|_: &Minimize, cx| with_shell(cx, |_, window, _| window.minimize_window()));
    cx.on_action(|_: &Zoom, cx| with_shell(cx, |_, window, _| window.zoom_window()));

    on_navigate::<GoOverview>(View::Overview, cx);
    on_navigate::<GoHoldings>(View::Holdings, cx);
    on_navigate::<GoAllocation>(View::Allocation, cx);
    on_navigate::<GoPerformance>(View::Performance, cx);
    on_navigate::<GoReview>(View::Review, cx);
    on_navigate::<GoRebalance>(View::Rebalance, cx);
    on_navigate::<GoPolicy>(View::Policy, cx);
    on_navigate::<GoDocuments>(View::Documents, cx);
    on_navigate::<GoBriefing>(View::Briefing, cx);
    on_navigate::<GoHealth>(View::Health, cx);
    on_navigate::<GoSettings>(View::Settings, cx);
}

fn toggle_privacy(cx: &mut App) {
    let privacy = cx.global_mut::<Privacy>();
    privacy.0 = !privacy.0;
    cx.refresh_windows();
}

/// A shortcut with the platform's main modifier.
macro_rules! key {
    ($key:literal) => {
        if cfg!(target_os = "macos") {
            concat!("cmd-", $key)
        } else {
            concat!("ctrl-", $key)
        }
    };
}

fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(key!("q"), Quit, None),
        KeyBinding::new(key!("o"), OpenFile, None),
        KeyBinding::new(key!("shift-o"), OpenWorkspace, None),
        KeyBinding::new(key!("s"), Save, None),
        KeyBinding::new(key!("r"), RefreshPrices, None),
        KeyBinding::new(key!("k"), CommandPalette, None),
        KeyBinding::new(key!("shift-h"), TogglePrivacy, None),
        KeyBinding::new(key!("m"), Minimize, None),
        KeyBinding::new(key!("1"), GoOverview, None),
        KeyBinding::new(key!("2"), GoHoldings, None),
        KeyBinding::new(key!("3"), GoAllocation, None),
        KeyBinding::new(key!("4"), GoPerformance, None),
        KeyBinding::new(key!("5"), GoReview, None),
        KeyBinding::new(key!("6"), GoRebalance, None),
        KeyBinding::new(key!("7"), GoPolicy, None),
        KeyBinding::new(key!("8"), GoDocuments, None),
        KeyBinding::new(key!("9"), GoBriefing, None),
        KeyBinding::new(key!("0"), GoHealth, None),
        KeyBinding::new(key!(","), GoSettings, None),
    ]);
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-alt-h", HideOthers, None),
    ]);
}

fn set_menus(cx: &mut App) {
    cx.set_menus([
        Menu::new("Stahl Capital").items([
            MenuItem::action("Settings…", GoSettings),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide Stahl Capital", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action("Quit Stahl Capital", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("Open Portfolio File…", OpenFile),
            MenuItem::action("Open Workspace…", OpenWorkspace),
            MenuItem::separator(),
            MenuItem::action("Save", Save),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("View").items([
            MenuItem::action("Command Palette…", CommandPalette),
            MenuItem::separator(),
            MenuItem::action("Refresh Prices", RefreshPrices),
            MenuItem::action("Hide or Show Amounts", TogglePrivacy),
            MenuItem::action("Toggle Light/Dark Theme", ToggleTheme),
        ]),
        Menu::new("Go").items([
            MenuItem::action("Overview", GoOverview),
            MenuItem::action("Holdings", GoHoldings),
            MenuItem::action("Allocation", GoAllocation),
            MenuItem::action("Performance", GoPerformance),
            MenuItem::separator(),
            MenuItem::action("Review", GoReview),
            MenuItem::action("Rebalance", GoRebalance),
            MenuItem::action("Policy", GoPolicy),
            MenuItem::action("Documents", GoDocuments),
            MenuItem::separator(),
            MenuItem::action("Briefing", GoBriefing),
            MenuItem::action("Data Health", GoHealth),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
        ]),
    ]);
}

fn command_item(label: &'static str, icon: IconName, action: impl Action) -> CommandItem {
    CommandItem::new()
        .label(label)
        .icon(icon)
        .action(Box::new(action))
}

fn open_command_palette(is_workspace: bool, window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    let state = cx.new(|cx| CommandState::new(window, cx));
    let focus = state.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .width(px(560.))
            .margin_top(px(96.))
            .close_button(false)
            .child(
                Command::new(&state)
                    .placeholder("Search pages and actions…")
                    .max_h(px(420.))
                    .bordered(false)
                    .group(CommandGroup::new().label("Go to").items([
                        command_item("Overview", View::Overview.icon(), GoOverview),
                        command_item("Holdings", View::Holdings.icon(), GoHoldings),
                        command_item("Allocation", View::Allocation.icon(), GoAllocation),
                        command_item("Performance", View::Performance.icon(), GoPerformance),
                        command_item("Briefing", View::Briefing.icon(), GoBriefing),
                        command_item("Data Health", View::Health.icon(), GoHealth),
                        command_item("Settings", View::Settings.icon(), GoSettings),
                    ]))
                    .when(is_workspace, |command| {
                        command.group(CommandGroup::new().label("Plan").items([
                            command_item("Review", View::Review.icon(), GoReview),
                            command_item("Rebalance", View::Rebalance.icon(), GoRebalance),
                            command_item("Policy", View::Policy.icon(), GoPolicy),
                            command_item("Documents", View::Documents.icon(), GoDocuments),
                        ]))
                    })
                    .group(CommandGroup::new().label("Actions").items([
                        command_item("Refresh Prices", IconName::RefreshCw, RefreshPrices),
                        command_item("Save", IconName::Save, Save),
                        command_item("Open Portfolio File…", IconName::FileBraces, OpenFile),
                        command_item("Open Workspace…", IconName::FolderOpen, OpenWorkspace),
                        command_item("Hide or Show Amounts", IconName::EyeOff, TogglePrivacy),
                        command_item("Toggle Light/Dark Theme", IconName::Moon, ToggleTheme),
                    ]))
                    .on_confirm(|_, window, cx| window.close_dialog(cx))
                    .on_cancel(|window, cx| window.close_dialog(cx)),
            )
    });
    focus.update(cx, |state, cx| state.focus(window, cx));
}

pub struct Shell {
    pub model: Entity<AppModel>,
    view: View,
    page: AnyView,
}

impl Shell {
    pub fn new(initial_file: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let model = cx.new(|cx| AppModel::new(initial_file, cx));
        cx.observe_in(&model, window, |shell, model, window, cx| {
            // Workspace pages go away with the workspace.
            if shell.view.needs_workspace() && !model.read(cx).is_workspace() {
                shell.navigate(View::Overview, window, cx);
            }
            window.set_window_edited(model.read(cx).dirty);
            cx.notify();
        })
        .detach();

        let shell = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            let dirty = shell
                .read_with(cx, |shell, cx| shell.model.read(cx).dirty)
                .unwrap_or(false);
            if dirty {
                let _ = shell.update(cx, |shell, cx| shell.close_or_ask(window, cx));
                false
            } else {
                // One window: closing it ends the app.
                cx.quit();
                true
            }
        });

        let page = pages::build(View::Overview, &model, window, cx);
        Self {
            model,
            view: View::Overview,
            page,
        }
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn navigate(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        if view.needs_workspace() && !self.model.read(cx).is_workspace() {
            return;
        }
        // Rebuilding on each visit fetches a page's data fresh.
        self.view = view;
        self.page = pages::build(view, &self.model, window, cx);
        cx.notify();
    }

    /// Quit, first offering to save position changes that are not on disk.
    fn close_or_ask(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.model.read(cx).dirty {
            cx.quit();
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            "Save changes to your portfolio before quitting?",
            Some("Your position changes are lost if you don't save them."),
            &["Save", "Don't Save", "Cancel"],
            cx,
        );
        let model = self.model.clone();
        cx.spawn(async move |_, cx| match answer.await {
            Ok(0) => {
                let backend = cx.update(|cx| cx.global::<Backend>().clone());
                let saving = backend.clone();
                let task = backend.run(async move { saving.save_portfolio().await });
                match task.await {
                    Ok(()) => cx.update(|cx| cx.quit()),
                    // Stay open: quitting now would lose what they asked to keep.
                    Err(error) => model.update(cx, |model, cx| {
                        model.error = Some(error);
                        cx.notify();
                    }),
                }
            }
            Ok(1) => cx.update(|cx| cx.quit()),
            _ => {}
        })
        .detach();
    }

    fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| {
            let next = match model.theme_mode() {
                ThemeMode::Dark => ThemeMode::Light,
                ThemeMode::Light => ThemeMode::Dark,
            };
            model.set_theme(next, cx);
        });
    }

    fn nav_item(&self, view: View, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let palette = cx.palette();
        let active = self.view == view;
        div()
            .id(view.id())
            .flex()
            .items_center()
            .w_full()
            .gap_2p5()
            .rounded_md()
            .px_2p5()
            .py_1p5()
            .text_sm()
            .text_color(if active {
                palette.fg_strong
            } else {
                palette.fg_muted
            })
            .when(active, |item| item.bg(palette.bg_tertiary))
            .when(!active, |item| {
                item.hover(|style| style.bg(palette.bg_tertiary).text_color(palette.fg))
            })
            .child(ui::icon(
                view.icon(),
                15.,
                if active {
                    palette.accent
                } else {
                    palette.fg_muted
                },
            ))
            .child(view.label())
            .on_click(cx.listener(move |shell, _, window, cx| shell.navigate(view, window, cx)))
            .test_support()
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let palette = cx.palette();
        let model = self.model.read(cx);
        let is_workspace = model.is_workspace();
        let document = match (&model.open.workspace_dir, &model.open.file_path) {
            (Some(dir), _) => Some(file_name(dir).to_string()),
            (None, Some(path)) => Some(file_name(path).to_string()),
            (None, None) => None,
        };
        let portfolio = View::PORTFOLIO.map(|view| self.nav_item(view, cx));
        let plan = View::PLAN.map(|view| self.nav_item(view, cx));
        let insights = View::INSIGHTS.map(|view| self.nav_item(view, cx));
        let settings = self.nav_item(View::Settings, cx);
        let unlock = div()
            .id("nav-unlock")
            .mx_2p5()
            .my_1()
            .text_xs()
            .text_color(palette.fg_subtle)
            .hover(|style| style.text_color(palette.accent))
            .child("Open a workspace to set a policy, review drift, and simulate rebalancing.")
            .on_click(cx.listener(|shell, _, _, cx| {
                shell.model.update(cx, |model, cx| model.open_workspace(cx));
            }))
            .test_support();

        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(px(220.))
            .h_full()
            .border_r_1()
            .border_color(palette.border)
            .bg(palette.bg_secondary)
            .pt(px(TITLEBAR_INSET))
            .child(
                ui::row()
                    .gap_2p5()
                    .px_4()
                    .pb_2()
                    .child(img(STAHL_MARK).size_7())
                    .child(
                        div()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette.fg_strong)
                                    .child("Stahl Capital"),
                            )
                            .children(document.map(|name| {
                                div()
                                    .text_xs()
                                    .truncate()
                                    .text_color(palette.fg_subtle)
                                    .child(name)
                            })),
                    ),
            )
            .child(
                div()
                    .id("sidebar-nav")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .px_2()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(nav_heading("Portfolio", cx))
                            .children(portfolio)
                            .child(nav_heading("Plan", cx))
                            .map(|nav| {
                                if is_workspace {
                                    nav.children(plan)
                                } else {
                                    nav.child(unlock)
                                }
                            })
                            .child(nav_heading("Insights", cx))
                            .children(insights),
                    ),
            )
            .child(div().p_2().child(settings))
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let palette = cx.palette();
        let hidden = cx.global::<Privacy>().0;
        let model = self.model.read(cx);
        let (loading, dirty, theme_mode) = (model.loading, model.dirty, model.theme_mode());
        ui::row()
            .flex_shrink_0()
            .h(px(52.))
            .px_4()
            .gap_2()
            .justify_between()
            .border_b_1()
            .border_color(palette.border)
            .child(
                ui::row()
                    .gap_3()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.fg_strong)
                            .child(self.view.label()),
                    )
                    .child(self.freshness(cx)),
            )
            .child(
                ui::row()
                    .flex_shrink_0()
                    .gap_1()
                    .when(dirty, |row| {
                        row.child(
                            Button::new("save")
                                .primary()
                                .small()
                                .icon(IconName::Save)
                                .label("Save Changes")
                                .tooltip("Your edits are not on disk yet")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.model.update(cx, |model, cx| model.save(cx));
                                })),
                        )
                    })
                    .child(
                        Button::new("palette")
                            .ghost()
                            .small()
                            .icon(IconName::Search)
                            .tooltip_with_action("Command Palette", &CommandPalette, None)
                            .on_click(cx.listener(|shell, _, window, cx| {
                                let is_workspace = shell.model.read(cx).is_workspace();
                                open_command_palette(is_workspace, window, cx);
                            })),
                    )
                    .child(
                        Button::new("privacy")
                            .ghost()
                            .small()
                            .icon(if hidden {
                                IconName::EyeOff
                            } else {
                                IconName::Eye
                            })
                            .tooltip_with_action(
                                if hidden {
                                    "Show Amounts"
                                } else {
                                    "Hide Amounts"
                                },
                                &TogglePrivacy,
                                None,
                            )
                            .on_click(|_, _, cx| toggle_privacy(cx)),
                    )
                    .child(
                        Button::new("toggle-theme")
                            .ghost()
                            .small()
                            .icon(match theme_mode {
                                ThemeMode::Dark => IconName::Sun,
                                ThemeMode::Light => IconName::Moon,
                            })
                            .tooltip(match theme_mode {
                                ThemeMode::Dark => "Switch to Light Mode",
                                ThemeMode::Light => "Switch to Dark Mode",
                            })
                            .on_click(cx.listener(|shell, _, _, cx| shell.toggle_theme(cx))),
                    )
                    .child(
                        Button::new("refresh-prices")
                            .ghost()
                            .small()
                            .icon(IconName::RefreshCw)
                            .loading(loading)
                            .tooltip_with_action("Refresh Prices", &RefreshPrices, None)
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell.model.update(cx, |model, cx| model.refresh_prices(cx));
                            })),
                    ),
            )
    }

    /// How much to trust the numbers on screen, at a glance.
    fn freshness(&self, cx: &App) -> impl IntoElement + use<> {
        let model = self.model.read(cx);
        let status = model
            .summary
            .as_ref()
            .map_or("complete", |summary| summary.data_status.as_str());
        let (label, tone) = freshness_label(&model.network, status);
        let text = match &model.updated_at {
            Some(time) => format!("{label} · {time}"),
            None => label.to_string(),
        };
        ui::row()
            .gap_1p5()
            .text_xs()
            .text_color(cx.palette().fg_muted)
            .child(div().size_1p5().rounded_full().bg(tone.color(cx)))
            .child(text)
    }

    fn main_area(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let error = self.model.read(cx).error.clone();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(self.toolbar(cx))
            .child(
                div()
                    .id("main")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .p_6()
                            .children(error.map(|error| ui::danger_banner(error, cx).mb_4()))
                            .child(self.page.clone()),
                    ),
            )
    }

    fn splash(&self, cx: &App) -> impl IntoElement + use<> {
        centered()
            .gap_4()
            .child(img(STAHL_MARK).size_12())
            .child(Spinner::new())
            .child(ui::muted("Loading your portfolio…", cx).text_sm())
    }

    fn welcome(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let error = self.model.read(cx).error.clone();
        let workspace = welcome_choice(
            "welcome-workspace",
            IconName::FolderOpen,
            true,
            "Open a workspace",
            "Recommended. Holds your portfolio, policy, diary, decisions, and reports.",
            cx,
        )
        .on_click(cx.listener(|shell, _, _, cx| {
            shell.model.update(cx, |model, cx| model.open_workspace(cx));
        }))
        .test_support();
        let file = welcome_choice(
            "welcome-file",
            IconName::FileBraces,
            false,
            "Open a positions file",
            "Quick start with just a positions.json. You can create a workspace later.",
            cx,
        )
        .on_click(cx.listener(|shell, _, _, cx| {
            shell.model.update(cx, |model, cx| model.open_file(cx));
        }))
        .test_support();

        centered()
            .p_6()
            .child(img(STAHL_MARK).size_16().mb_6())
            .child(
                div()
                    .mb_2()
                    .text_3xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.palette().fg_strong)
                    .child("Welcome to Stahl Capital"),
            )
            .child(ui::muted("Your local-first portfolio, on your own machine.", cx).mb_8())
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_4()
                    .w_full()
                    .max_w(px(672.))
                    .child(workspace)
                    .child(file),
            )
            .child(ui::caption("or drop a file or folder anywhere on this window", cx).mt_6())
            .children(error.map(|error| ui::danger_banner(error, cx).mt_6().max_w(px(672.))))
    }

    /// Shown instead of an empty portfolio when the file does not parse.
    fn parse_failure(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let palette = cx.palette();
        let problem = self.model.read(cx).open.parse_error.clone();
        let (file, reason, location) = problem.map_or_else(
            || (String::new(), String::new(), String::new()),
            |problem| {
                (
                    problem.file,
                    problem.reason,
                    format!("Line {}, column {}", problem.line, problem.column),
                )
            },
        );
        centered().p_6().child(
            ui::card(cx)
                .w_full()
                .max_w(px(640.))
                .gap_3()
                .child(
                    ui::row()
                        .gap_2()
                        .child(ui::icon(IconName::TriangleAlert, 20., palette.warning))
                        .child(ui::section_title("This portfolio can't be read")),
                )
                .child(
                    ui::muted(
                        "Nothing has been changed. Fix the file in your editor, then reload.",
                        cx,
                    )
                    .text_sm(),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .rounded_lg()
                        .bg(tint(palette.warning))
                        .p_3()
                        .text_sm()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette.warning)
                                .child(location),
                        )
                        .child(reason)
                        .child(ui::caption(file, cx)),
                )
                .child(
                    ui::row()
                        .gap_2()
                        .child(
                            Button::new("reload-file")
                                .primary()
                                .icon(IconName::RefreshCw)
                                .label("Reload")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.model.update(cx, |model, cx| model.reload(cx));
                                })),
                        )
                        .child(
                            Button::new("open-other")
                                .secondary()
                                .label("Open Another File…")
                                .on_click(cx.listener(|shell, _, _, cx| {
                                    shell.model.update(cx, |model, cx| model.open_file(cx));
                                })),
                        ),
                ),
        )
    }
}

/// What to call the state of the prices, from `portfolio_rs`'s network status
/// (connected, partial, offline, fixture) and data status (complete, partial,
/// stale).
fn freshness_label(network: &str, data_status: &str) -> (&'static str, ui::Tone) {
    match (network, data_status) {
        ("offline", _) => ("Offline", ui::Tone::Warning),
        (_, "stale") => ("Stale prices", ui::Tone::Warning),
        ("partial", _) | (_, "partial") => ("Partial prices", ui::Tone::Warning),
        ("fixture", _) => ("Fixture prices", ui::Tone::Info),
        _ => ("Live", ui::Tone::Success),
    }
}

fn centered() -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .size_full()
}

fn nav_heading(title: &'static str, cx: &App) -> impl IntoElement + use<> {
    div()
        .px_2p5()
        .pt_4()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.palette().fg_subtle)
        .child(title)
}

fn welcome_choice(
    id: &'static str,
    icon: IconName,
    primary: bool,
    title: &'static str,
    description: &'static str,
    cx: &App,
) -> Stateful<Div> {
    let palette = cx.palette();
    let (icon_bg, icon_color) = if primary {
        (palette.accent, palette.accent_fg)
    } else {
        (palette.bg_tertiary, palette.fg)
    };
    ui::card(cx)
        .id(id)
        .items_start()
        .hover(|style| style.border_color(palette.accent))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .mb_4()
                .size_10()
                .rounded_lg()
                .bg(icon_bg)
                .child(ui::icon(icon, 20., icon_color)),
        )
        .child(ui::section_title(title).mb_1())
        .child(ui::muted(description, cx).text_sm())
}

fn file_name(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
}

/// Copy text a page offers for pasting elsewhere, such as into an AI chat.
pub fn copy_text(text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        let (starting, has_portfolio, broken) = {
            let model = self.model.read(cx);
            (
                model.starting,
                model.has_portfolio(),
                model.open.parse_error.is_some(),
            )
        };
        let root = div()
            .id("shell")
            .relative()
            .flex()
            .size_full()
            .overflow_hidden()
            .bg(palette.bg)
            .text_color(palette.fg)
            .on_drop(cx.listener(|shell, paths: &ExternalPaths, _, cx| {
                if let Some(path) = paths.paths().first() {
                    let path = path.clone();
                    shell
                        .model
                        .update(cx, |model, cx| model.open_dropped(&path, cx));
                }
            }))
            .drag_over::<ExternalPaths>(move |style, _, _, _| {
                style.border_2().border_color(palette.accent)
            });

        if starting {
            root.child(self.splash(cx)).into_any_element()
        } else if broken {
            root.child(self.parse_failure(cx)).into_any_element()
        } else if !has_portfolio {
            root.child(self.welcome(cx)).into_any_element()
        } else {
            root.child(self.sidebar(cx))
                .child(self.main_area(cx))
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_never_calls_doubtful_prices_live() {
        assert_eq!(freshness_label("connected", "complete").0, "Live");
        assert_eq!(freshness_label("offline", "complete").0, "Offline");
        assert_eq!(freshness_label("connected", "stale").0, "Stale prices");
        assert_eq!(freshness_label("partial", "complete").0, "Partial prices");
        assert_eq!(freshness_label("connected", "partial").0, "Partial prices");
        assert_eq!(freshness_label("fixture", "complete").0, "Fixture prices");
    }

    #[test]
    fn file_names_come_from_either_path_style() {
        assert_eq!(file_name("/home/me/positions.json"), "positions.json");
        assert_eq!(file_name("/home/me/portfolio/"), "portfolio");
        assert_eq!(file_name("C:\\Users\\me\\positions.json"), "positions.json");
    }
}
