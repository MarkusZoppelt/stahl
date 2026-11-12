//! The prepared matters on the Overview, the seal they are signed with, and the decision
//! records on the Documents page.
//!
//! The section is a pure function of [`State`]. The hold itself is state: a press starts it,
//! letting go cancels it, and [`State::matters_tick`] completes it on the clock. The ring is
//! drawn from the clock, so it fills at the frame rate and stops when the finger does.

use std::f32::consts::{FRAC_PI_2, TAU};

use chrono::NaiveDate;
use rune::{
    Color, Cursor, Cx, Drag, DragItem, El, Key, KeyEvent, Phase, Shape, Spring, SpringPair,
    TextEdit, Transition, Tx, canvas, col, el, icon, row, text,
};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, Markdown, TextInput};

use crate::{
    agenda::{Action, Calm, Item, Weight},
    format,
    fx::Plate,
    icons,
    matters::{self, Matter, Proposal, Stamp},
    state::{Page, State},
    theme::{FIGURE, NUMBER, SECTION, Theme},
    view::{rebalance, ui},
};

/// The Sign control: a round steel control a finger can rest on.
const SIGN: f32 = 58.0;
/// The seal's diameter.
const SEAL: f32 = 40.0;
/// The ring's stroke, and how far inside the control it sits.
const RING: f32 = 3.0;
/// A card presses in this much while its Sign control is held.
const PRESS: f32 = 0.006;
/// The give of a card springing back after a cancelled hold.
const SPRING_BACK: Spring = Spring::new(0.24, 0.55);
/// A seal coming down onto paper.
const STAMP_DOWN: Spring = Spring::new(0.30, 0.62);

fn weight_color(weight: Weight, t: &Theme) -> Color {
    match weight {
        // Attention, not loss: no matter here is about a loss, so none borrows its colour.
        Weight::Total | Weight::Critical | Weight::Warning => t.warn,
        Weight::Notice => t.info,
    }
}

/// The matter's small heading, in the colour of what it weighs.
fn heading(matter: &Matter, t: &Theme) -> El {
    row()
        .center_y()
        .gap(8.0)
        .child(ui::dot(weight_color(matter.weight, t)))
        .child(ui::caption(matter.kind.label().to_uppercase(), t))
}

/// A trade, as the plan would make it: what it does, in money, to which class, and where that
/// leaves the class's share.
fn trade_row(trade: &matters::TradeLine, before: f64, after: f64, money: &str, t: &Theme) -> El {
    let k = &t.kit;
    row()
        .center_y()
        .w_full()
        .gap(10.0)
        .child(
            row()
                .center_y()
                .grow(1.0)
                .w(0.0)
                .min_w(0.0)
                .gap(6.0)
                .child(
                    text(if trade.buy { "Buy" } else { "Sell" })
                        .font(NUMBER)
                        .color(k.c.text_muted),
                )
                .child(text(money).font(NUMBER).color(k.c.text_base))
                .child(
                    text(trade.class.clone())
                        .font(k.ty.ui)
                        .color(k.c.text_base)
                        .truncate()
                        .shrink(),
                ),
        )
        .child(
            text(format!("{before:.1}% → {after:.1}%"))
                .font(NUMBER)
                .color(k.c.text_muted),
        )
}

/// The proposal: the trades with their figures, and the plan in full when it is opened.
fn proposal(matter: &Matter, s: &State, t: &Theme, narrow: bool, signed: bool) -> El {
    let k = &t.kit;
    let Proposal {
        trades, rows, note, ..
    } = &matter.proposal;
    if trades.is_empty() {
        return col();
    }
    let expanded = s.matters.expanded.as_deref() == Some(matter.id.as_str());
    let trade_cell = |class: &str| {
        trades
            .iter()
            .find(|trade| trade.class == class)
            .map_or_else(
                || ui::number("-", k.c.text_muted),
                |trade| {
                    ui::number(
                        format!(
                            "{} {}",
                            if trade.buy { "Buy" } else { "Sell" },
                            s.money(trade.amount)
                        ),
                        k.c.text_base,
                    )
                },
            )
    };
    let trade_rows = trades.iter().map(|trade| {
        let (before, after) = rows
            .iter()
            .find(|row| row.class == trade.class)
            .map_or((0.0, 0.0), |row| (row.before, row.after));
        trade_row(trade, before, after, &s.money(trade.amount), t)
    });
    // Opened, the plan is one table: today, the trade, and where each class ends up. Closed,
    // it is just the trades, so the card can be read in ten seconds.
    let plan = expanded.then(|| {
        if narrow {
            ui::table(
                &["Asset class", "Trade", "After"],
                rows.iter()
                    .map(|row| {
                        vec![
                            text(row.class.clone()).font(k.ty.ui).color(k.c.text_base),
                            trade_cell(&row.class),
                            ui::number(format!("{:.1}%", row.after), k.c.text_muted),
                        ]
                    })
                    .collect(),
                t,
            )
        } else {
            ui::table(
                &["Asset class", "Today", "Trade", "After", "Target"],
                rows.iter()
                    .map(|row| {
                        vec![
                            text(row.class.clone()).font(k.ty.ui).color(k.c.text_base),
                            ui::number(format!("{:.1}%", row.before), k.c.text_muted),
                            trade_cell(&row.class),
                            ui::number(format!("{:.1}%", row.after), k.c.text_base),
                            ui::number(
                                row.target.map_or_else(
                                    || "-".to_string(),
                                    |target| format!("{target:.1}%"),
                                ),
                                k.c.text_muted,
                            ),
                        ]
                    })
                    .collect(),
                t,
            )
        }
    });
    let toggle = {
        let id = matter.id.clone();
        Button::new(if expanded {
            "Hide the plan"
        } else {
            "The plan"
        })
        .variant(ButtonVariant::Ghost)
        .icon(if expanded {
            &icons::CHEVRON_LEFT
        } else {
            &icons::CHEVRON_RIGHT
        })
        .control_size(ControlSize::Small)
        .on_press(move |s: &mut State| {
            s.matters.expanded = if s.matters.expanded.as_deref() == Some(id.as_str()) {
                None
            } else {
                Some(id.clone())
            };
        })
        .build(k)
        .key(format!("matter-plan-{}", matter.id))
    };
    let body = if expanded {
        col().w_full().gap(8.0).children(plan)
    } else {
        col().w_full().gap(4.0).children(trade_rows)
    };
    col()
        .w_full()
        .gap(8.0)
        .child(body)
        .child(ui::caption(note.clone(), t).wrap())
        .children((!signed).then(|| {
            row()
                .center_y()
                .w_full()
                .gap(8.0)
                .child(toggle)
                .child(el().grow(1.0))
        }))
}

/// The ring around the Sign control: a track, and the part of it the hold has filled.
fn hold_ring(progress: f32, t: &Theme) -> El {
    let centre = SIGN / 2.0;
    let radius = centre - RING;
    let arc = |to: f32| -> Vec<[f32; 2]> {
        (0..=72)
            .map(|i| {
                let angle = -FRAC_PI_2 + to * TAU * i as f32 / 72.0;
                [centre + radius * angle.cos(), centre + radius * angle.sin()]
            })
            .collect()
    };
    let mut shape = canvas(Shape::new().stroke(arc(1.0), RING, t.kit.c.layer_03))
        .absolute(0.0, 0.0)
        .size(SIGN, SIGN)
        .a11y_hidden();
    if progress > 0.002 {
        shape = canvas(
            Shape::new()
                .stroke(arc(1.0), RING, t.kit.c.layer_03)
                .stroke(arc(progress), RING, t.accent),
        )
        .absolute(0.0, 0.0)
        .size(SIGN, SIGN)
        .a11y_hidden();
        // The head of the ring catches the light, as the live end of a chart does.
        let angle = -FRAC_PI_2 + progress * TAU;
        let head = [centre + radius * angle.cos(), centre + radius * angle.sin()];
        return el()
            .size(SIGN, SIGN)
            .child(shape)
            .child(ui::glow_orb(t.accent, 10.0, 0.9).absolute(head[0] - 5.0, head[1] - 5.0));
    }
    shape
}

/// The Sign control. A press and hold of about a second records the decision; letting go
/// sooner springs everything back and says how it works. The keyboard does the same on Return
/// or Space. Under reduced motion there is no hold: a press asks plainly.
fn sign_control(matter: &Matter, s: &State, t: &Theme, reduced: bool) -> El {
    let k = &t.kit;
    let id = matter.id.clone();
    let holding = s.matters.holding(&matter.id);
    let progress = if holding {
        s.matters.progress(s.now).unwrap_or(0.0)
    } else {
        0.0
    };
    let mut control = el()
        .key(format!("matter-sign-{}", matter.id))
        .size(SIGN, SIGN)
        .radius(SIGN / 2.0)
        .center()
        .role("button")
        .label(format!("Sign: {}", matter.title))
        .focusable(true)
        .cursor(Cursor::Pointer)
        .fill(if holding {
            t.accent.alpha(0.10)
        } else {
            t.sunken
        })
        .border(1.0, if holding { t.accent } else { k.c.border_base })
        .hover(k.c.hover)
        .help(if reduced {
            "Sign this decision"
        } else {
            "Hold to sign"
        });
    if reduced {
        control = control.on_press(move |s: &mut State| {
            s.matters.confirm_sign = Some(id.clone());
        });
    } else {
        let press_id = id.clone();
        let key_id = id.clone();
        control = control
            .on_drag(
                move |s: &mut State, drag: &mut Drag, _: &mut Tx| match drag.phase {
                    Phase::Down => s.begin_hold(&press_id, false),
                    Phase::Up => s.release_hold(),
                    Phase::Move => {}
                },
            )
            .on_key(move |s: &mut State, event: &KeyEvent, _: &mut Tx| {
                if !is_sign_key(event) {
                    return false;
                }
                s.begin_hold(&key_id, true);
                true
            })
            .on_key_up(|s: &mut State, event: &KeyEvent, _: &mut Tx| {
                if !is_sign_key(event) {
                    return false;
                }
                s.release_hold();
                true
            });
    }
    control.child(hold_ring(progress, t)).child(
        text("Sign")
            .font(k.ty.ui_medium)
            .color(if holding { t.accent } else { k.c.text_base }),
    )
}

/// Return or Space, the keyboard's way to hold the Sign control.
fn is_sign_key(event: &KeyEvent) -> bool {
    event.key == Key::Enter || matches!(&event.key, Key::Text(text) if text == " ")
}

/// The seal of a matter: a disc of turned steel with a raised rim, lit by the room's one light.
/// Unsigned it is blank; signed it carries the date, struck into the metal. Under reduced motion
/// it arrives in its final state with no movement.
pub fn seal(s: &State, date: Option<NaiveDate>, size: f32, t: &Theme, reduced: bool) -> El {
    let k = &t.kit;
    let radius = size / 2.0;
    let (ink, lip) = ui::steel_ink();
    let transition = if reduced || date.is_none() {
        Transition::none()
    } else {
        Transition::scale(1.35)
            .combined(Transition::opacity())
            .springs(SpringPair::new(STAMP_DOWN, STAMP_DOWN))
    };
    let disc = ui::plate(s, t, Plate::new(0.8), radius)
        .key(if date.is_some() {
            "matter-seal"
        } else {
            "matter-seal-blank"
        })
        .size(size, size)
        .radius(radius)
        .continuous_corners()
        .center()
        .a11y_hidden()
        .transition(transition)
        // The raised rim: lit on the side facing the light, in shade on the other.
        .child(
            el().absolute(0.0, 0.0)
                .size(size, size)
                .radius(radius)
                .border(1.5, lip)
                .a11y_hidden(),
        )
        .child(
            el().absolute(5.0, 5.0)
                .size(size - 10.0, size - 10.0)
                .radius((size - 10.0) / 2.0)
                .border(1.0, ink.alpha(0.28))
                .a11y_hidden(),
        );
    match date {
        Some(date) => {
            // The day over the month, each cut into the metal on its own line.
            let stamp = matters::seal_date(date);
            let (day, month) = stamp.split_once(' ').unwrap_or((stamp.as_str(), ""));
            let word = |words: &str| {
                row().justify_center().w(size - 12.0).child(ui::engraved(
                    words.to_string(),
                    k.ty.text_xsmall.tracking(0.4),
                    ink,
                    lip,
                ))
            };
            disc.child(col().center().child(word(day)).child(word(month)))
        }
        None => disc,
    }
}

/// Decline asks for an optional line, so the same matter is not proposed again for the same
/// reason.
fn declining(matter: &Matter, s: &State, t: &Theme) -> Option<El> {
    let k = &t.kit;
    let declining = s
        .matters
        .declining
        .as_ref()
        .filter(|declining| declining.id == matter.id)?;
    let id = matter.id.clone();
    let decline_id = id.clone();
    Some(
        col()
            .w_full()
            .gap(8.0)
            .mt(2.0)
            .child(ui::wide_field(
                "Reason (optional)",
                TextInput::new(&declining.reason)
                    .placeholder("One line, if you like.")
                    .width(None)
                    .on_edit(|s: &mut State| {
                        &mut s
                            .matters
                            .declining
                            .as_mut()
                            .expect("the decline form is open")
                            .reason
                    })
                    .build(k)
                    .key(format!("matter-reason-{}", matter.id)),
                t,
            ))
            .child(
                row()
                    .center_y()
                    .gap(8.0)
                    .child(
                        Button::new("Decline")
                            .on_press(move |s: &mut State| {
                                let reason = s
                                    .matters
                                    .declining
                                    .as_ref()
                                    .filter(|declining| declining.id == decline_id)
                                    .map_or_else(String::new, |declining| {
                                        declining.reason.text.clone()
                                    });
                                s.decline_matter(&decline_id, reason);
                            })
                            .build(k)
                            .key(format!("matter-decline-yes-{}", matter.id)),
                    )
                    .child(
                        Button::new("Cancel")
                            .variant(ButtonVariant::Ghost)
                            .on_press(|s: &mut State| s.matters.declining = None)
                            .build(k)
                            .key(format!("matter-decline-no-{}", matter.id)),
                    ),
            ),
    )
}

/// One prepared matter: what it is, why, the plan, and the three things the owner can do.
fn matter_card(
    matter: &Matter,
    index: usize,
    s: &State,
    t: &Theme,
    narrow: bool,
    reduced: bool,
    stamp: Option<&Stamp>,
) -> El {
    let k = &t.kit;
    let signed = stamp.is_some();
    let holding = s.matters.holding(&matter.id);
    let progress = if holding {
        s.matters.progress(s.now).unwrap_or(0.0)
    } else {
        0.0
    };
    let scale = 1.0 - PRESS * progress;
    let mut card = ui::card(t)
        .key(format!("matter-{}", matter.id))
        .w_full()
        .padding(if narrow { 14.0 } else { 18.0 })
        .gap(10.0)
        .scale(scale)
        .scale_motion(SPRING_BACK);
    if signed {
        card = card.border(1.0, t.accent.alpha(0.45)).transition(
            // It is already on the page; when the breath is over it files itself away upward.
            Transition::asymmetric(
                Transition::none(),
                Transition::offset(0.0, -40.0).combined(Transition::opacity()),
            ),
        );
    }
    // The seal takes the Adjust button's place when the matter is signed, so the head never
    // changes height and the card never jumps.
    let adjust = || {
        matter.adjust.map(|action| {
            Button::new("Adjust")
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .on_press(move |s: &mut State| open_plan(s, action))
                .build(k)
                .key(format!("matter-adjust-{}", matter.id))
                .transition(Transition::none())
        })
    };
    // On a wide screen Sign and the seal sit in the head, beside Adjust, so they are on the
    // first screen however long the plan is. A phone keeps Sign under the card.
    let right = match stamp {
        Some(stamp) => seal(s, Some(stamp.date), SEAL, t, reduced),
        None if narrow => adjust().unwrap_or_else(el),
        None => row()
            .center_y()
            .gap(12.0)
            .children(adjust())
            .child(sign_control(matter, s, t, reduced))
            .child(seal(s, None, SEAL, t, reduced)),
    };
    let head = row()
        .key("head")
        .center_y()
        .w_full()
        .justify_between()
        .gap(12.0)
        .child(heading(matter, t))
        .child(right);
    let title = text(matter.title.clone())
        .key("title")
        .font(SECTION)
        .color(k.c.text_base)
        .wrap();
    let signed_line = stamp.map(|stamp| {
        row()
            .key("signed-line")
            .center_y()
            .gap(8.0)
            .transition(ui::appear())
            .child(text("Signed").font(FIGURE).color(t.up))
            .child(
                text(matters::long_date(stamp.date))
                    .font(FIGURE)
                    .color(k.c.text_muted),
            )
    });
    let reasoning = text(matter.reasoning.clone())
        .key("reasoning")
        .font(k.ty.ui)
        .color(k.c.text_base)
        .wrap()
        .max_w(760.0);
    let prior = col()
        .key("prior")
        .w_full()
        .pt(2.0)
        .children(matter.prior.clone().map(|prior| {
            row()
                .items_start()
                .gap(8.0)
                .child(icon(&icons::LANDMARK, 14.0).color(k.c.text_faint).mt(2.0))
                .child(
                    text(prior)
                        .font(k.ty.text_xsmall)
                        .color(k.c.text_muted)
                        .wrap()
                        .shrink(),
                )
        }));
    let proposal = proposal(matter, s, t, narrow, signed).key("proposal");
    let records = ui::caption(
        if signed {
            "Recorded in your documents.".to_string()
        } else {
            matter.records.clone()
        },
        t,
    )
    .key("records")
    .wrap();
    let footer = if stamp.is_some() {
        el().key("actions")
    } else {
        let hint = s
            .matters
            .hint
            .as_ref()
            .filter(|(id, _)| id == &matter.id)
            .map(|_| ui::caption("Hold to sign.", t).transition(ui::appear()));
        let others = row()
            .center_y()
            .gap(4.0)
            .children(hint)
            .child(decline_button(matter, t))
            .child(postpone_button(matter, t));
        let actions = if narrow {
            col()
                .w_full()
                .gap(10.0)
                .items_start()
                .child(sign_control(matter, s, t, reduced))
                .child(others)
        } else {
            row().center_y().w_full().gap(12.0).child(others)
        };
        actions.key("actions").transition(Transition::none())
    };
    card = card
        .child(head)
        .child(title)
        .children(signed_line)
        .child(reasoning)
        .child(prior)
        .child(proposal)
        .child(records);
    if let Some(form) = declining(matter, s, t) {
        return card.child(form);
    }
    if signed {
        card
    } else {
        let card = card
            .child(ui::rule(t).key("rule").transition(Transition::none()))
            .child(footer);
        ui::entering(card, index)
            .transition(Transition::asymmetric(ui::arrive(), Transition::opacity()))
    }
}

fn open_plan(s: &mut State, action: Action) {
    if let Action::Open(Page::Rebalance) = action {
        // The plan the matter proposes is the one the page opens on.
        rebalance::set_blend(s, matters::HALFWAY);
    }
    action.perform(s);
}

fn decline_button(matter: &Matter, t: &Theme) -> El {
    let id = matter.id.clone();
    Button::new("Decline")
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press(move |s: &mut State| {
            s.matters.declining = Some(matters::Declining {
                id: id.clone(),
                reason: TextEdit::new(""),
            });
            s.matters.hint = None;
        })
        .build(&t.kit)
        .key(format!("matter-decline-{}", matter.id))
}

fn postpone_button(matter: &Matter, t: &Theme) -> El {
    let id = matter.id.clone();
    Button::new("Put off")
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press(move |s: &mut State| s.postpone_matter(&id))
        .build(&t.kit)
        .key(format!("matter-postpone-{}", matter.id))
}

/// A data fix: lighter than a matter, with the one action that deals with it.
fn todo_row(item: &Item, index: usize, t: &Theme, narrow: bool) -> El {
    let k = &t.kit;
    let action = item.action;
    let words = col()
        .min_w(0.0)
        .gap(2.0)
        .child(
            text(item.title.clone())
                .font(k.ty.ui)
                .color(k.c.text_base)
                .wrap(),
        )
        .children(
            item.detail
                .clone()
                .map(|detail| ui::caption(detail, t).wrap()),
        );
    let words = if narrow {
        words.w_full()
    } else {
        words.grow(1.0).w(0.0)
    };
    let button = Button::new(action.label())
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press(move |s: &mut State| action.perform(s))
        .build(k)
        .key(format!("todo-action-{}", item.id));
    let body = if narrow {
        col()
            .grow(1.0)
            .w(0.0)
            .min_w(0.0)
            .gap(6.0)
            .items_start()
            .child(words)
            .child(button)
    } else {
        row()
            .center_y()
            .grow(1.0)
            .w(0.0)
            .min_w(0.0)
            .gap(12.0)
            .child(words)
            .child(button)
    };
    row()
        .key(format!("todo-{}", item.id))
        .w_full()
        .items_start()
        .gap(10.0)
        .py(if narrow { 6.0 } else { 9.0 })
        .children((index > 0).then(|| ui::rule(t).absolute(0.0, 0.0)))
        .child(ui::dot(t.kit.c.text_faint).mt(6.0))
        .child(body)
}

/// What the section says when there is nothing to sign: one sentence, and the true detail the
/// agenda has always given.
fn quiet(s: &State, t: &Theme, with_detail: bool) -> El {
    let k = &t.kit;
    let detail = match s.agenda_calm() {
        // Nothing can be claimed either way until the review lands.
        Calm::Checking => {
            return ui::muted("Checking against your policy…", t).key("matters-checking");
        }
        Calm::Clear(words) => words,
    };
    row()
        .key("matters-quiet")
        .w_full()
        .items_start()
        .gap(8.0)
        .child(icon(&icons::CHECK_CIRCLE, 16.0).color(t.up).mt(2.0))
        .child(
            col()
                .min_w(0.0)
                .gap(2.0)
                .child(
                    text("Nothing needs your signature today. The vault is quiet.")
                        .font(k.ty.ui)
                        .color(k.c.text_base)
                        .wrap(),
                )
                .children(with_detail.then(|| ui::caption(detail, t).wrap())),
        )
}

/// For your signature, and below it, lighter, what needs doing.
pub fn section(s: &State, t: &Theme, narrow: bool, reduced: bool) -> El {
    let prepared = s.prepared();
    let mut section = ui::ledger(t)
        .key("agenda")
        .child(ui::title("For your signature", t))
        .children(
            s.matters
                .ledger_problem
                .as_ref()
                .map(|problem| ui::danger(problem.clone(), t).key("matters-ledger-problem")),
        );
    let mut cards: Vec<El> = prepared
        .matters
        .iter()
        .enumerate()
        .map(|(index, matter)| matter_card(matter, index, s, t, narrow, reduced, None))
        .collect();
    // A matter that was just signed keeps its place, stamped, until it files itself away. It
    // is the same card, so nothing moves: the seal comes down where the buttons were.
    if let Some(stamp) = s
        .matters
        .stamp
        .as_ref()
        .filter(|stamp| s.now - stamp.started < matters::BREATH)
    {
        let at = stamp.index.min(cards.len());
        cards.insert(
            at,
            matter_card(&stamp.matter, at, s, t, narrow, reduced, Some(stamp)),
        );
    }
    if let Some(why) = s.matters_withheld().filter(|_| cards.is_empty()) {
        // Nothing can be signed on these figures, and the section says why instead of "quiet".
        section = section.child(
            row()
                .key("matters-withheld")
                .w_full()
                .child(ui::muted(why, t).wrap()),
        );
    } else if cards.is_empty() {
        // The detail is only true when the agenda itself is quiet: a signed or declined matter
        // leaves the list, but the portfolio it was about may still be out of band.
        section = section.child(quiet(
            s,
            t,
            prepared.todo.is_empty() && s.agenda().is_empty(),
        ));
    } else {
        section = section.child(col().key("matter-cards").w_full().gap(12.0).children(cards));
    }
    // The reduced-motion path: a plain question instead of the hold.
    if !prepared.todo.is_empty() {
        section = section.child(
            ui::ledger(t)
                .key("todo")
                .child(ui::title("To do", t))
                .child(
                    col().w_full().children(
                        prepared
                            .todo
                            .iter()
                            .enumerate()
                            .map(|(index, item)| todo_row(item, index, t, narrow)),
                    ),
                ),
        );
    }
    section.children(
        s.matters
            .error
            .as_ref()
            .filter(|error| Some(*error) != s.matters.ledger_problem.as_ref())
            .map(|error| ui::danger(error.clone(), t)),
    )
}

/// The reduced-motion confirmation, drawn over the page as the app's other dialogs are.
pub fn confirm_dialog(s: &State, t: &Theme, layer: &mut Vec<El>) {
    let Some(id) = s.matters.confirm_sign.as_deref() else {
        return;
    };
    let prepared = s.prepared();
    let Some(matter) = prepared.matters.iter().find(|matter| matter.id == id) else {
        return;
    };
    let k = &t.kit;
    let id = matter.id.clone();
    Dialog::new("matter-confirm", "Sign this decision?")
        .open(true)
        .fit()
        .width(460.0)
        .description(format!(
            "{}. Signing records it in your documents. Stahl never trades.",
            matter.title
        ))
        .on_close(|s: &mut State| s.matters.confirm_sign = None)
        .child(
            row()
                .center_y()
                .w_full()
                .justify_end()
                .px(16.0)
                .pb(16.0)
                .gap(8.0)
                .child(
                    Button::new("Cancel")
                        .on_press(|s: &mut State| s.matters.confirm_sign = None)
                        .build(k)
                        .key("matter-confirm-cancel"),
                )
                .child(
                    Button::new("Sign")
                        .variant(ButtonVariant::Submit)
                        .on_press(move |s: &mut State| {
                            s.matters.confirm_sign = None;
                            s.sign_matter(&id);
                        })
                        .build(k)
                        .key("matter-confirm-sign"),
                ),
        )
        .build(k, layer);
}

/// The decision records on the Documents page, newest first, with the one opened below them.
pub fn records(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let k = &t.kit;
    let room = ui::room(s, cx);
    let narrow = room == ui::Room::Narrow;
    let list = if s.matters.decisions.is_empty() {
        ui::caption(
            if s.matters.decisions_loaded {
                "No decisions yet. Signing a matter writes one here."
            } else {
                "Reading your decisions…"
            },
            t,
        )
        .key("decision-empty")
    } else {
        col().w_full().children(
            s.matters
                .decisions
                .iter()
                .enumerate()
                .map(|(index, decision)| {
                    let path = decision.path.to_string_lossy().into_owned();
                    let status_color = match decision.status.as_str() {
                        "Signed" => t.up,
                        "Declined" => t.warn,
                        _ => t.kit.c.text_muted,
                    };
                    let date = NaiveDate::parse_from_str(&decision.date, "%Y-%m-%d")
                        .map_or_else(|_| decision.date.clone(), matters::long_date);
                    let words = if narrow {
                        col()
                            .w_full()
                            .min_w(0.0)
                            .gap(4.0)
                            .child(
                                row()
                                    .center_y()
                                    .w_full()
                                    .justify_between()
                                    .gap(8.0)
                                    .child(text(date).font(NUMBER).color(k.c.text_muted))
                                    .child(ui::badge(decision.status.clone(), status_color)),
                            )
                            .child(
                                text(decision.title.clone())
                                    .font(k.ty.ui_medium)
                                    .color(k.c.text_base)
                                    .wrap(),
                            )
                    } else {
                        row()
                            .center_y()
                            .w_full()
                            .gap(12.0)
                            .child(text(date).font(NUMBER).color(k.c.text_muted))
                            .child(
                                text(decision.title.clone())
                                    .font(k.ty.ui_medium)
                                    .color(k.c.text_base)
                                    .truncate()
                                    .shrink()
                                    .grow(1.0),
                            )
                            .child(ui::badge(decision.status.clone(), status_color))
                    };
                    row()
                        .key(format!("decision-{}", decision.key()))
                        .w_full()
                        .center_y()
                        .gap(12.0)
                        .py(8.0)
                        .radius(6.0)
                        .px(6.0)
                        .mx(-6.0)
                        .role("button")
                        .focusable(true)
                        .cursor(Cursor::Pointer)
                        .hover(k.c.hover)
                        .on_press(move |s: &mut State| {
                            s.matters.open_decision = Some(path.clone());
                        })
                        .children((index > 0).then(|| ui::rule(t).absolute(0.0, 0.0)))
                        .child(words)
                })
                .collect::<Vec<_>>(),
        )
    };
    let opened = s
        .matters
        .open_decision
        .as_ref()
        .and_then(|path| {
            s.matters
                .decisions
                .iter()
                .find(|decision| decision.path.to_string_lossy() == path.as_str())
        })
        .map(|decision| {
            let path = decision.path.to_string_lossy().into_owned();
            let copy = decision.body.clone();
            let saved = Some(decision.path.clone());
            ui::entering(
                col()
                    .key("decision-open")
                    .w_full()
                    .gap(8.0)
                    .pt(4.0)
                    .child(
                        row()
                            .center_y()
                            .w_full()
                            .justify_between()
                            .gap(12.0)
                            .child(
                                ui::caption(format::place(&path), t)
                                    .truncate_middle()
                                    .shrink()
                                    .drag_out(move |_: &State| {
                                        Some(DragItem::new().file(path.clone()))
                                    }),
                            )
                            .child(ui::copy_button("copy-decision", copy.clone(), t)),
                    )
                    .child(
                        col()
                            .w_full()
                            .padding(16.0)
                            .radius(8.0)
                            .fill(t.sunken)
                            .children(if s.privacy {
                                vec![ui::muted("Amounts are hidden.", t)]
                            } else {
                                vec![Markdown::new(copy).selectable(true).build(k)]
                            }),
                    )
                    .children(saved.map(|path| {
                        ui::caption(
                            format!("Saved in {}", format::place(&path.to_string_lossy())),
                            t,
                        )
                        .truncate_middle()
                    })),
                0,
            )
        });
    ui::card(t)
        .key("decision-records")
        .w_full()
        .gap(10.0)
        .child(ui::header(
            "Decision records",
            "What you signed and declined, newest first. Stahl never trades; a record is a \
             decision on a plan, not an order.",
            t,
        ))
        .child(list)
        .children(opened)
        .children(
            s.matters
                .error
                .as_ref()
                .map(|error| ui::danger(error.clone(), t)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seal_carries_the_date_in_the_bank_s_short_form() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
        assert_eq!(matters::seal_date(date), "3 OCT");
        assert_eq!(matters::long_date(date), "3 October 2026");
    }
}
