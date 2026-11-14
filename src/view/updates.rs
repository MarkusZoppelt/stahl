//! Telling someone about a new version, and installing it once they say so.

use std::path::Path;

use rune::{El, ScrollSpec, Tx, col, icon, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Markdown, Spinner};

use crate::{
    icons,
    state::{State, UpdatePhase},
    theme::Theme,
    update::{Plan, Release, install_command},
    view::ui,
};

pub fn build(s: &State, t: &Theme, layer: &mut Vec<El>) {
    let Some(release) = &s.updates.available else {
        return;
    };
    if s.updates.dialog {
        dialog(s, release, t, layer);
    } else if !s.updates.dismissed {
        layer.push(banner(s, release, t));
    }
}

/// In a corner, on every screen, and in nobody's way.
fn banner(s: &State, release: &Release, t: &Theme) -> El {
    let k = &t.kit;
    let (heading, detail, action) = if s.updates.phase == UpdatePhase::Installed {
        (
            format!("Stahl Capital {} is installed", release.version),
            "Restart to start using it.".to_string(),
            "Restart…",
        )
    } else {
        (
            format!("Stahl Capital {} is available", release.version),
            format!("You have {}.", s.updates.updater.current()),
            "View Update",
        )
    };
    ui::card(t)
        .key("update-banner")
        .role("status")
        .label(heading.clone())
        .pin(None, None, Some(24.0), Some(24.0))
        .w(340.0)
        .padding(14.0)
        .gap(10.0)
        .fill(k.c.layer_01)
        .transition(ui::arrive())
        .child(
            row()
                .center_y()
                .gap(10.0)
                .child(icon(&icons::DOWNLOAD, 16.0).color(t.accent))
                .child(
                    col()
                        .gap(1.0)
                        .grow(1.0)
                        .min_w(0.0)
                        .child(ui::title(heading, t).wrap())
                        .child(ui::caption(detail, t)),
                ),
        )
        .child(
            row()
                .center_y()
                .w_full()
                .justify_end()
                .gap(8.0)
                .child(
                    Button::new("Later")
                        .on_press(|s: &mut State| s.dismiss_update())
                        .build(k)
                        .key("update-dismiss"),
                )
                .child(
                    Button::new(action)
                        .variant(ButtonVariant::Submit)
                        .on_press(|s: &mut State| s.open_update_dialog())
                        .build(k)
                        .key("update-view"),
                ),
        )
}

fn notes(release: &Release, t: &Theme) -> El {
    let k = &t.kit;
    let body = if release.notes.trim().is_empty() {
        ui::caption("This release has no notes.", t)
    } else {
        Markdown::new(release.notes.clone())
            .selectable(true)
            .on_link_tx(|s: &mut State, url, tx: &mut Tx| s.open_notes_link(url, tx))
            .build(k)
    };
    col()
        .key("update-notes")
        .w_full()
        .max_h(240.0)
        .min_h(0.0)
        .padding(12.0)
        .radius(8.0)
        .border(1.0, k.c.border_muted)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(body)
}

fn later_button(label: &str, t: &Theme) -> El {
    Button::new(label)
        .on_press(|s: &mut State| s.dismiss_update())
        .build(&t.kit)
        .key("update-later")
}

fn page_button(t: &Theme) -> El {
    Button::new("View Release")
        .icon(&icons::EXTERNAL_LINK)
        .on_press_tx(|s: &mut State, tx: &mut Tx| s.open_release_page(tx))
        .build(&t.kit)
        .key("update-page")
}

fn buttons(children: Vec<El>) -> El {
    row()
        .center_y()
        .w_full()
        .justify_end()
        .gap(8.0)
        .children(children)
}

/// What pressing the button will do, said before they press it.
fn offer(plan: &Plan, t: &Theme) -> Vec<El> {
    let k = &t.kit;
    let verified = "It's checked against the SHA-256 checksum published with the release.";
    match plan {
        Plan::Replace { .. } => vec![
            ui::caption(
                format!(
                    "This downloads the new version and replaces this app. You choose when it \
                     restarts. {verified}"
                ),
                t,
            )
            .wrap(),
            buttons(vec![
                later_button("Later", t),
                page_button(t),
                Button::new("Install Update")
                    .variant(ButtonVariant::Submit)
                    .icon(&icons::DOWNLOAD)
                    .on_press(|s: &mut State| s.install_update())
                    .build(k)
                    .key("update-install"),
            ]),
        ],
        Plan::SavePackage { .. } => vec![
            ui::caption(
                format!(
                    "Installing a system package needs an administrator, so Stahl won't do it \
                     itself. This saves the .deb in your downloads, for you to install. {verified}"
                ),
                t,
            )
            .wrap(),
            buttons(vec![
                later_button("Later", t),
                page_button(t),
                Button::new("Download Package")
                    .variant(ButtonVariant::Submit)
                    .icon(&icons::DOWNLOAD)
                    .on_press(|s: &mut State| s.install_update())
                    .build(k)
                    .key("update-install"),
            ]),
        ],
        Plan::Link { why } => vec![
            ui::caption(*why, t).wrap(),
            buttons(vec![later_button("Later", t), page_button(t)]),
        ],
    }
}

fn saved(path: &Path, t: &Theme) -> Vec<El> {
    let command = install_command(path);
    vec![
        ui::notice(
            format!("Verified and saved to {}. To install it:", path.display()),
            t,
        ),
        text(command.clone())
            .font(t.kit.ty.ui)
            .color(t.kit.c.text_base)
            .selectable(true)
            .wrap(),
        buttons(vec![
            later_button("Done", t),
            Button::new("Copy Command")
                .icon(&icons::COPY)
                .on_press_tx(move |s: &mut State, tx: &mut Tx| s.copy(command.clone(), tx))
                .build(&t.kit)
                .key("update-copy"),
        ]),
    ]
}

fn dialog(s: &State, release: &Release, t: &Theme, layer: &mut Vec<El>) {
    let k = &t.kit;
    let current = s.updates.updater.current();
    let plan = s.updates.updater.plan(release);
    let parts: Vec<El> = match &s.updates.phase {
        UpdatePhase::Idle => offer(&plan, t),
        UpdatePhase::Installing => vec![
            row()
                .center_y()
                .gap(10.0)
                .key("update-progress")
                .role("status")
                .child(Spinner::new().size(14.0).build(k))
                .child(ui::muted("Downloading and verifying…", t)),
        ],
        UpdatePhase::Installed => {
            let label = if s.dirty {
                "Save and Restart"
            } else {
                "Restart Now"
            };
            let detail = if s.dirty {
                "Your unsaved changes are saved first."
            } else {
                "You can keep working: the new version starts next time you open the app."
            };
            let mut parts = vec![
                ui::notice(format!("Version {} is installed.", release.version), t),
                ui::caption(detail, t).wrap(),
            ];
            parts.extend(
                s.updates
                    .restart_error
                    .as_ref()
                    .map(|error| ui::danger(error.clone(), t)),
            );
            parts.push(buttons(vec![
                later_button("Later", t),
                Button::new(label)
                    .variant(ButtonVariant::Submit)
                    .icon(&icons::REFRESH)
                    .on_press(|s: &mut State| s.restart_after_update())
                    .build(k)
                    .key("update-restart"),
            ]));
            parts
        }
        UpdatePhase::Saved(path) => saved(path, t),
        UpdatePhase::Failed(error) => vec![
            ui::danger(error.clone(), t),
            ui::caption(
                "Nothing was changed, and this version keeps working. You can try again, or \
                 download it yourself from the release page.",
                t,
            )
            .wrap(),
            buttons(vec![
                later_button("Close", t),
                page_button(t),
                Button::new("Try Again")
                    .variant(ButtonVariant::Submit)
                    .on_press(|s: &mut State| s.install_update())
                    .build(k)
                    .key("update-retry"),
            ]),
        ],
    };
    let body = col()
        .w_full()
        .gap(12.0)
        .px(16.0)
        .pb(16.0)
        .child(notes(release, t))
        .children(parts);
    Dialog::new("update", format!("Stahl Capital {}", release.version))
        .open(true)
        .fit()
        .width(560.0)
        .description(format!("You have {current}."))
        .on_close(|s: &mut State| {
            s.close_update_dialog();
        })
        .child(body)
        .build(k, layer);
}

/// Where to check by hand, and what an update would mean for this copy.
pub fn settings_card(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let updates = &s.updates;
    let found = updates.available.as_ref().map(|release| {
        Button::new(format!("View {}", release.version))
            .icon(&icons::DOWNLOAD)
            .on_press(|s: &mut State| {
                s.updates.dismissed = false;
                s.open_update_dialog();
            })
            .build(k)
            .key("settings-update-view")
    });
    let check = if updates.checking {
        row()
            .center_y()
            .gap(8.0)
            .key("settings-update-checking")
            .child(Spinner::new().size(14.0).build(k))
            .child(ui::caption("Checking…", t))
    } else {
        Button::new("Check for Updates")
            .icon(&icons::REFRESH)
            .on_press(|s: &mut State| s.check_for_updates(false))
            .build(k)
            .key("settings-check-updates")
    };
    ui::card(t)
        .key("settings-updates")
        .child(ui::header(
            "Updates",
            format!(
                "Version {}. {}",
                updates.updater.current(),
                updates.updater.installation().describe()
            ),
            t,
        ))
        .child(row().center_y().gap(8.0).child(check).children(found))
}
