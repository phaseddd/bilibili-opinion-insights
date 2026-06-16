use gpui::{
    FontWeight, Hsla, IntoElement, ParentElement, Styled as _, div, hsla, linear_color_stop,
    linear_gradient, px, relative, rgb,
};
use gpui_component::{h_flex, v_flex};

use crate::gui::components::jelly_progress::JellyProgressPhase;
use crate::gui::materials::JellyMaterialToken;
use crate::gui::state::events::EventKind;
use crate::gui::state::task::TaskPhase;
use crate::gui::theme::Palette;

pub(crate) fn panel(palette: &Palette) -> gpui::Div {
    v_flex()
        .p(px(16.))
        .rounded(px(14.))
        .border_1()
        .border_color(palette.border)
        .bg(palette.surface)
        .shadow_sm()
}

pub(crate) fn panel_title(
    title: &'static str,
    subtitle: &'static str,
    palette: &Palette,
) -> impl IntoElement {
    v_flex()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(palette.muted)
                .child(subtitle),
        )
}

pub(crate) fn form_section(
    label: &'static str,
    helper: &'static str,
    input: impl IntoElement,
    palette: &Palette,
) -> impl IntoElement {
    v_flex()
        .gap(px(8.))
        .child(section_label(label, palette))
        .child(input)
        .child(
            div()
                .text_size(px(11.))
                .text_color(palette.muted)
                .line_height(relative(1.25))
                .child(helper),
        )
}

pub(crate) fn option_group(
    label: &'static str,
    content: impl IntoElement,
    palette: &Palette,
) -> impl IntoElement {
    v_flex()
        .gap(px(8.))
        .child(section_label(label, palette))
        .child(content)
}

pub(crate) fn section_label(label: &'static str, palette: &Palette) -> impl IntoElement {
    div()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette.text)
        .child(label)
}

pub(crate) fn validation_box(message: &str, palette: &Palette) -> impl IntoElement {
    h_flex()
        .gap(px(8.))
        .items_start()
        .p(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(palette.error.opacity(0.35))
        .bg(palette.error.opacity(0.07))
        .child(status_dot(palette.error))
        .child(
            div()
                .text_size(px(12.))
                .line_height(relative(1.3))
                .text_color(palette.error)
                .child(message.to_string()),
        )
}

pub(crate) fn metric_chip(
    label: &'static str,
    value: usize,
    color: Hsla,
    palette: &Palette,
) -> impl IntoElement {
    v_flex()
        .flex_1()
        .gap(px(5.))
        .p(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(color.opacity(0.2))
        .bg(color.opacity(0.07))
        .child(
            div()
                .text_size(px(11.))
                .text_color(palette.muted)
                .child(label),
        )
        .child(
            div()
                .text_size(px(17.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.text)
                .child(value.to_string()),
        )
}

pub(crate) fn status_badge(
    label: &'static str,
    kind: EventKind,
    palette: &Palette,
) -> impl IntoElement {
    let color = event_color(kind, palette);
    let material = JellyMaterialToken::for_event(kind, palette);
    h_flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(6.))
        .px(px(9.))
        .py(px(5.))
        .rounded(px(999.))
        .border_1()
        .border_color(material.rim.opacity(0.34))
        .bg(linear_gradient(
            135.,
            linear_color_stop(material.shell_start.opacity(0.13), 0.0),
            linear_color_stop(material.shell_end.opacity(0.16), 1.0),
        ))
        .shadow(vec![gpui::BoxShadow {
            color: material.state_aura.opacity(0.10),
            offset: gpui::point(px(0.), px(5.)),
            blur_radius: px(12.),
            spread_radius: px(-7.),
        }])
        .child(status_dot(color))
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(label),
        )
}

pub(crate) fn product_mark(palette: &Palette) -> impl IntoElement {
    div()
        .relative()
        .size(px(40.))
        .rounded(px(13.))
        .overflow_hidden()
        .bg(linear_gradient(
            135.,
            linear_color_stop(palette.accent_2, 0.0),
            linear_color_stop(palette.accent, 1.0),
        ))
        .shadow_md()
        .child(
            div()
                .absolute()
                .top(px(7.))
                .left(px(8.))
                .size(px(14.))
                .rounded(px(999.))
                .bg(hsla(0., 0., 1., 0.55)),
        )
        .child(
            div()
                .absolute()
                .right(px(7.))
                .bottom(px(6.))
                .size(px(12.))
                .rounded(px(999.))
                .bg(hsla(0., 0., 1., 0.72)),
        )
}

pub(crate) fn status_dot(color: Hsla) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .mt(px(2.))
        .size(px(8.))
        .rounded(px(999.))
        .bg(color)
        .shadow_sm()
}

pub(crate) fn event_color(kind: EventKind, palette: &Palette) -> Hsla {
    match kind {
        EventKind::System => palette.muted,
        EventKind::Video => palette.accent_2,
        EventKind::Comments => palette.accent_2,
        EventKind::Danmaku => palette.accent,
        EventKind::Output => rgb(0x5865f2).into(),
        EventKind::Warning => palette.warning,
        EventKind::Success => palette.success,
        EventKind::Failure => palette.error,
    }
}

pub(crate) fn phase_kind(phase: TaskPhase) -> EventKind {
    match phase {
        TaskPhase::Idle => EventKind::System,
        TaskPhase::Validating => EventKind::Warning,
        TaskPhase::Running => EventKind::Danmaku,
        TaskPhase::Cancelling => EventKind::Warning,
        TaskPhase::Completed => EventKind::Success,
        TaskPhase::Failed => EventKind::Failure,
    }
}

pub(crate) fn progress_visual_phase(phase: TaskPhase) -> JellyProgressPhase {
    match phase {
        TaskPhase::Idle => JellyProgressPhase::Idle,
        TaskPhase::Validating => JellyProgressPhase::Validating,
        TaskPhase::Running => JellyProgressPhase::Running,
        TaskPhase::Cancelling => JellyProgressPhase::Cancelling,
        TaskPhase::Completed => JellyProgressPhase::Completed,
        TaskPhase::Failed => JellyProgressPhase::Failed,
    }
}
