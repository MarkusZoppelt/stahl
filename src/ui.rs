//! Layout pieces shared by the pages.

use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, Hsla, IntoElement, ParentElement as _, Rgba,
    SharedString, Styled as _, Window,
    assets::IconName,
    component::{
        Icon, Sizable as _,
        button::{Button, ButtonVariants as _},
        spinner::Spinner,
    },
    div, px, relative,
};

use crate::{
    backend::{Backend, BackendResult},
    theme::{ActivePalette as _, tint},
};

// --- Talking to the backend ---

/// Run backend work off the UI thread, then hand the result to the view.
pub fn run_backend<V, T, F>(
    cx: &mut Context<V>,
    work: impl FnOnce(Backend) -> F,
    done: impl FnOnce(&mut V, BackendResult<T>, &mut Context<V>) + 'static,
) where
    V: 'static,
    T: Send + 'static,
    F: Future<Output = BackendResult<T>> + Send + 'static,
{
    let backend = cx.global::<Backend>().clone();
    let task = backend.run(work(backend.clone()));
    cx.spawn(async move |view, cx| {
        let result = task.await;
        let _ = view.update(cx, |view, cx| {
            done(view, result, cx);
            cx.notify();
        });
    })
    .detach();
}

/// [`run_backend`], for views that update text inputs with the result.
pub fn run_backend_in<V, T, F>(
    window: &mut Window,
    cx: &mut Context<V>,
    work: impl FnOnce(Backend) -> F,
    done: impl FnOnce(&mut V, BackendResult<T>, &mut Window, &mut Context<V>) + 'static,
) where
    V: 'static,
    T: Send + 'static,
    F: Future<Output = BackendResult<T>> + Send + 'static,
{
    let backend = cx.global::<Backend>().clone();
    let task = backend.run(work(backend.clone()));
    cx.spawn_in(window, async move |view, cx| {
        let result = task.await;
        let _ = view.update_in(cx, |view, window, cx| {
            done(view, result, window, cx);
            cx.notify();
        });
    })
    .detach();
}

/// Data a page fetches when it opens. A failed reload keeps the last result.
pub struct Loadable<T> {
    pub data: Option<T>,
    pub loading: bool,
    pub error: Option<String>,
}

impl<T> Default for Loadable<T> {
    fn default() -> Self {
        Self {
            data: None,
            loading: false,
            error: None,
        }
    }
}

impl<T> Loadable<T> {
    pub fn finish(&mut self, result: BackendResult<T>) {
        self.loading = false;
        match result {
            Ok(data) => {
                self.data = Some(data);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    /// The error banner, or a loading card until there is data to show.
    pub fn status(&self, loading_label: &'static str, cx: &App) -> Option<Div> {
        match &self.error {
            Some(error) => Some(danger_banner(error.clone(), cx)),
            None if self.loading && self.data.is_none() => Some(loading_card(loading_label, cx)),
            None => None,
        }
    }
}

// --- Layout ---

pub fn stack() -> Div {
    div().flex().flex_col().gap_6()
}

pub fn row() -> Div {
    div().flex().items_center()
}

/// Columns that fit the window.
pub fn grid_columns(window: &Window, wide: u16, narrow: u16) -> u16 {
    if window.viewport_size().width >= px(1180.) {
        wide
    } else {
        narrow
    }
}

pub fn grid(columns: u16, gap: f32) -> Div {
    div().grid().grid_cols(columns).gap(px(gap))
}

pub fn card(cx: &App) -> Div {
    let palette = cx.palette();
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .rounded_xl()
        .border_1()
        .border_color(palette.border)
        .bg(palette.bg_secondary)
        .p_5()
}

pub fn card_header(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    cx: &App,
) -> Div {
    div()
        .child(section_title(title))
        .child(muted(description, cx).text_sm())
}

pub fn section_title(title: impl Into<SharedString>) -> Div {
    div()
        .text_base()
        .font_weight(FontWeight::SEMIBOLD)
        .child(title.into())
}

pub fn muted(text: impl Into<SharedString>, cx: &App) -> Div {
    div().text_color(cx.palette().fg_muted).child(text.into())
}

/// A small muted label over a figure.
pub fn caption(text: impl Into<SharedString>, cx: &App) -> Div {
    muted(text, cx).text_xs()
}

/// A labelled figure.
pub fn stat(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    tone: Option<Tone>,
    cx: &App,
) -> Div {
    let color = tone.map_or(cx.palette().fg_strong, |tone| tone.color(cx));
    div()
        .flex()
        .flex_col()
        .gap_1()
        .min_w_0()
        .child(caption(label, cx))
        .child(
            div()
                .text_xl()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(value.into()),
        )
}

pub fn stat_card(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    tone: Option<Tone>,
    cx: &App,
) -> Div {
    card(cx).p_4().child(stat(label, value, tone, cx))
}

/// A bar showing a share of the whole, with an optional target tick.
pub fn share_bar(percent: f64, target: Option<f64>, color: impl Into<Hsla>, cx: &App) -> Div {
    let palette = cx.palette();
    let fraction = |value: f64| (value / 100.0).clamp(0.0, 1.0) as f32;
    div()
        .relative()
        .w_full()
        .h(px(6.))
        .rounded_full()
        .bg(palette.bg_tertiary)
        .child(
            div()
                .absolute()
                .h_full()
                .rounded_full()
                .bg(color.into())
                .w(relative(fraction(percent))),
        )
        .children(target.map(|target| {
            div()
                .absolute()
                .top(px(-3.))
                .h(px(12.))
                .w(px(2.))
                .rounded_full()
                .bg(palette.fg_strong)
                .left(relative(fraction(target)))
        }))
}

/// One share per slice, side by side.
pub fn stacked_bar(shares: &[(f64, Rgba)], cx: &App) -> Div {
    div()
        .flex()
        .w_full()
        .h(px(8.))
        .gap(px(2.))
        .rounded_full()
        .overflow_hidden()
        .bg(cx.palette().bg_tertiary)
        .children(
            shares
                .iter()
                .filter(|(percent, _)| *percent > 0.0)
                .map(|(percent, color)| {
                    div()
                        .h_full()
                        .bg(*color)
                        .w(relative((*percent / 100.0).clamp(0.0, 1.0) as f32))
                }),
        )
}

pub fn icon(name: IconName, size: f32, color: impl Into<Hsla>) -> Icon {
    Icon::new(name).size(px(size)).text_color(color.into())
}

// --- Status ---

#[derive(Clone, Copy)]
pub enum Tone {
    Success,
    Danger,
    Warning,
    Info,
}

impl Tone {
    pub fn color(self, cx: &App) -> Rgba {
        let palette = cx.palette();
        match self {
            Tone::Success => palette.success,
            Tone::Danger => palette.danger,
            Tone::Warning => palette.warning,
            Tone::Info => palette.info,
        }
    }

    /// Green for gains and flat, red for losses.
    pub fn of_change(value: f64) -> Self {
        if value >= 0.0 {
            Tone::Success
        } else {
            Tone::Danger
        }
    }
}

pub fn badge(text: impl Into<SharedString>, tone: Tone, cx: &App) -> Div {
    let color = tone.color(cx);
    row()
        .flex_shrink_0()
        .gap_1()
        .rounded_full()
        .px_2()
        .py_0p5()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(color)
        .bg(tint(color))
        .child(text.into())
}

pub fn danger_banner(text: impl Into<SharedString>, cx: &App) -> Div {
    let danger = cx.palette().danger;
    div()
        .rounded_lg()
        .border_1()
        .border_color(danger)
        .bg(tint(danger))
        .px_4()
        .py_3()
        .text_sm()
        .text_color(danger)
        .child(text.into())
}

pub fn message_banner(text: impl Into<SharedString>, cx: &App) -> Div {
    let palette = cx.palette();
    div()
        .rounded_lg()
        .border_1()
        .border_color(palette.border)
        .bg(palette.bg_tertiary)
        .px_4()
        .py_3()
        .text_sm()
        .child(text.into())
}

pub fn loading_card(label: &'static str, cx: &App) -> Div {
    card(cx)
        .flex_row()
        .items_center()
        .justify_center()
        .gap_2()
        .py_16()
        .text_sm()
        .text_color(cx.palette().fg_muted)
        .child(Spinner::new().small())
        .child(label)
}

pub fn bullet_list(items: &[String], color: impl Into<Hsla>) -> Div {
    let color = color.into();
    div()
        .flex()
        .flex_col()
        .gap_1()
        .text_sm()
        .text_color(color)
        .children(items.iter().map(|item| {
            div()
                .flex()
                .gap_2()
                .child("•")
                .child(div().flex_1().child(SharedString::from(item.clone())))
        }))
}

/// A finding from `portfolio_rs`, in its severity's color.
pub fn finding_row(
    tone: Tone,
    title: impl Into<SharedString>,
    detail: Option<SharedString>,
    cx: &App,
) -> Div {
    let color = tone.color(cx);
    div()
        .flex()
        .gap_3()
        .rounded_lg()
        .bg(tint(color))
        .p_3()
        .child(
            div()
                .flex_shrink_0()
                .mt_1()
                .size_2()
                .rounded_full()
                .bg(color),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(title.into()),
                )
                .children(detail.map(|detail| caption(detail, cx).mt_0p5())),
        )
}

// --- Buttons ---

pub fn copy_markdown_button(id: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .icon(IconName::Copy)
        .label("Copy as Markdown")
        .tooltip("For pasting into notes or an AI assistant")
}

pub fn refresh_button(id: &'static str, loading: bool) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .icon(IconName::RefreshCw)
        .label("Refresh")
        .loading(loading)
}

// --- Tables ---

#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Right,
}

pub struct Column {
    pub title: &'static str,
    pub align: Align,
}

pub const fn left(title: &'static str) -> Column {
    Column {
        title,
        align: Align::Left,
    }
}

pub const fn right(title: &'static str) -> Column {
    Column {
        title,
        align: Align::Right,
    }
}

fn cell(column: &Column, content: AnyElement) -> Div {
    let cell = div().flex().items_center().flex_1().min_w_0();
    match column.align {
        Align::Left => cell.justify_start(),
        Align::Right => cell.justify_end(),
    }
    .child(content)
}

pub fn table(columns: &[Column], rows: Vec<Vec<AnyElement>>, cx: &App) -> Div {
    let palette = cx.palette();
    div()
        .flex()
        .flex_col()
        .text_sm()
        .child(
            row().gap_3().pb_2().text_color(palette.fg_muted).children(
                columns
                    .iter()
                    .map(|column| cell(column, div().child(column.title).into_any_element())),
            ),
        )
        .children(rows.into_iter().map(|cells| {
            row()
                .gap_3()
                .py_2()
                .border_t_1()
                .border_color(palette.border)
                .children(
                    columns
                        .iter()
                        .zip(cells)
                        .map(|(column, content)| cell(column, content)),
                )
        }))
}

pub fn text_cell(text: impl Into<SharedString>) -> AnyElement {
    div().child(text.into()).into_any_element()
}
