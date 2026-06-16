use gpui::{
    Bounds, Corners, FontWeight, Hsla, IntoElement, ParentElement, Styled as _, Window, canvas,
    div, hsla, linear_color_stop, linear_gradient, point, px, relative, rgb, size,
};
use gpui_component::{h_flex, v_flex};

use crate::gui::components::jelly_progress::JellyProgressPhase;
use crate::gui::materials::JellyMaterialToken;
use crate::gui::rendering::jelly_image_cache::{JellyCapsuleImage, JellySurfaceImage};
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
    image: Option<JellySurfaceImage>,
) -> impl IntoElement {
    let has_image = image.is_some();
    let backing_alpha = if has_image { 0.035 } else { 0.07 };
    let border_alpha = if has_image { 0.16 } else { 0.2 };
    let chip = v_flex()
        .relative()
        .flex_1()
        .gap(px(5.))
        .p(px(10.))
        .rounded(px(10.))
        .overflow_hidden()
        .border_1()
        .border_color(color.opacity(border_alpha))
        .bg(color.opacity(backing_alpha));

    let chip = if let Some(image) = image {
        chip.child(
            canvas(
                move |_, _window: &mut Window, _cx| (),
                move |bounds, _, window: &mut Window, _cx| {
                    let origin_x = f32::from(bounds.origin.x);
                    let origin_y = f32::from(bounds.origin.y);
                    let width = f32::from(bounds.size.width);
                    let height = f32::from(bounds.size.height);
                    let image_bounds = Bounds::new(
                        point(px(origin_x), px(origin_y)),
                        size(px(width), px(height)),
                    );
                    let _ = window.paint_image(
                        image_bounds,
                        Corners::from(px(10.)),
                        image.image.clone(),
                        0,
                        false,
                    );
                },
            )
            .absolute()
            .inset_0(),
        )
    } else {
        chip
    };

    chip.child(
        div()
            .relative()
            .text_size(px(11.))
            .text_color(palette.muted)
            .child(label),
    )
    .child(
        div()
            .relative()
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
    status_capsule(label, kind, palette, None)
}

pub(crate) fn status_capsule(
    label: &'static str,
    kind: EventKind,
    palette: &Palette,
    image: Option<JellyCapsuleImage>,
) -> impl IntoElement {
    let color = event_color(kind, palette);
    let material = JellyMaterialToken::for_event(kind, palette);
    let has_image = image.is_some();
    let border_alpha = if has_image { 0.34 } else { 0.42 };
    let shell_shadow_scale = if has_image { 0.6 } else { 1.0 };
    let shell_backing_alpha = if has_image { 0.22 } else { 0.16 };
    let inner_shadow_alpha = if has_image { 0.12 } else { 0.18 };
    let dot_scale: f32 = if has_image { 1.16 } else { 1.0 };
    let label_alpha = if has_image { 0.98 } else { 1.0 };
    let capsule = h_flex()
        .relative()
        .flex_shrink_0()
        .items_center()
        .gap(px(7.))
        .min_h(px(32.))
        .px(px(11.))
        .py(px(6.))
        .rounded(px(999.))
        .overflow_hidden()
        .border_1()
        .border_color(material.rim.opacity(border_alpha))
        .bg(linear_gradient(
            135.,
            linear_color_stop(material.shell_start.opacity(0.12), 0.0),
            linear_color_stop(material.shell_end.opacity(0.18), 1.0),
        ))
        .shadow(vec![
            gpui::BoxShadow {
                color: material.state_aura.opacity(0.12 * shell_shadow_scale),
                offset: gpui::point(px(0.), px(6.)),
                blur_radius: px(14.),
                spread_radius: px(-8.),
            },
            gpui::BoxShadow {
                color: material
                    .inner_glow
                    .opacity(inner_shadow_alpha * shell_shadow_scale),
                offset: gpui::point(px(0.), px(2.)),
                blur_radius: px(10.),
                spread_radius: px(-5.),
            },
        ])
        .child(
            div()
                .absolute()
                .left(px(3.))
                .right(px(3.))
                .bottom(px(1.))
                .h(px(14.))
                .rounded(px(999.))
                .bg(material.contact_shadow.opacity(0.12 * shell_shadow_scale)),
        )
        .child(
            div()
                .absolute()
                .left(px(9.))
                .right(px(13.))
                .top(px(3.))
                .h(px(2.))
                .rounded(px(999.))
                .bg(material.specular.opacity(0.34)),
        );

    if let Some(image) = image {
        capsule
            .child(
                canvas(
                    move |_, _window: &mut Window, _cx| (),
                    move |bounds, _, window: &mut Window, _cx| {
                        let origin_x = f32::from(bounds.origin.x);
                        let origin_y = f32::from(bounds.origin.y);
                        let width = f32::from(bounds.size.width);
                        let height = f32::from(bounds.size.height);
                        let image_bounds = Bounds::new(
                            point(px(origin_x), px(origin_y)),
                            size(px(width), px(height)),
                        );
                        let _ = window.paint_image(
                            image_bounds,
                            Corners::from(px(height * 0.5)),
                            image.image.clone(),
                            0,
                            false,
                        );
                    },
                )
                .absolute()
                .inset_0(),
            )
            .child(
                div()
                    .absolute()
                    .left(px(3.))
                    .right(px(6.))
                    .bottom(px(1.))
                    .h(px(14.))
                    .rounded(px(999.))
                    .bg(material.contact_shadow.opacity(0.06)),
            )
            .child(
                div()
                    .relative()
                    .flex_shrink_0()
                    .size(px((9. * dot_scale).round().max(8.)))
                    .rounded(px(999.))
                    .border_1()
                    .border_color(material.rim.opacity(0.3))
                    .bg(linear_gradient(
                        135.,
                        linear_color_stop(color.opacity(0.92), 0.0),
                        linear_color_stop(material.specular.opacity(0.42), 1.0),
                    ))
                    .shadow(vec![gpui::BoxShadow {
                        color: material.state_aura.opacity(0.22),
                        offset: gpui::point(px(0.), px(2.)),
                        blur_radius: px(6.),
                        spread_radius: px(-2.),
                    }]),
            )
            .child(
                div()
                    .relative()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color.opacity(label_alpha))
                    .child(label),
            )
    } else {
        capsule
            .child(
                div()
                    .absolute()
                    .left(px(3.))
                    .right(px(3.))
                    .bottom(px(1.))
                    .h(px(14.))
                    .rounded(px(999.))
                    .bg(material.contact_shadow.opacity(shell_backing_alpha)),
            )
            .child(
                div()
                    .relative()
                    .flex_shrink_0()
                    .size(px(9.))
                    .rounded(px(999.))
                    .border_1()
                    .border_color(material.rim.opacity(0.34))
                    .bg(linear_gradient(
                        135.,
                        linear_color_stop(color.opacity(0.92), 0.0),
                        linear_color_stop(material.specular.opacity(0.42), 1.0),
                    ))
                    .shadow(vec![gpui::BoxShadow {
                        color: material.state_aura.opacity(0.26),
                        offset: gpui::point(px(0.), px(2.)),
                        blur_radius: px(6.),
                        spread_radius: px(-2.),
                    }]),
            )
            .child(
                div()
                    .relative()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color)
                    .child(label),
            )
    }
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
