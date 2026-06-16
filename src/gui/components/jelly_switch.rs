use gpui::{
    FontWeight, Hsla, InteractiveElement as _, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, hsla, linear_color_stop, linear_gradient,
    px, relative, rgb,
};

use crate::gui::motion::wave_between;
use crate::gui::theme::Palette;

#[derive(Clone, Copy)]
pub enum JellySwitchTone {
    Primary,
    Cyan,
    Output,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum JellySwitchSize {
    Standard,
    Compact,
}

#[derive(Clone, Copy)]
pub struct JellySwitchConfig {
    pub label: &'static str,
    pub checked: bool,
    pub enabled: bool,
    pub tone: JellySwitchTone,
    pub size: JellySwitchSize,
    pub motion_tick: u64,
    pub group: &'static str,
    pub id_seed: usize,
    pub active: bool,
}

pub fn jelly_switch(config: JellySwitchConfig, palette: &Palette) -> gpui::Div {
    let (track_w, track_h, thumb, text_size) = match config.size {
        JellySwitchSize::Standard => (132., 48., 42., 12.),
        JellySwitchSize::Compact => (96., 34., 29., 11.),
    };
    let opacity = if config.enabled { 1. } else { 0.46 };
    let progress = if config.checked { 1. } else { 0. };
    let pulse = if config.active {
        wave_between(config.motion_tick, 0.18, 0.06, 0.18)
    } else {
        0.
    };
    let wobble = if config.active {
        ((config.motion_tick as f32 * 0.47).sin()) * 1.7
    } else {
        0.
    };
    let travel = track_w - thumb - 8.;
    let thumb_left = 4. + travel * progress + if config.checked { wobble } else { -wobble };
    let (start, end, aura, text): (Hsla, Hsla, Hsla, Hsla) = match config.tone {
        JellySwitchTone::Primary => (
            palette.accent_2,
            palette.accent,
            palette.accent,
            rgb(0x075b70).into(),
        ),
        JellySwitchTone::Cyan => (
            palette.accent,
            rgb(0x77e6f1).into(),
            palette.accent,
            rgb(0x075c67).into(),
        ),
        JellySwitchTone::Output => (
            rgb(0x9ee8ff).into(),
            rgb(0xffd7e8).into(),
            palette.accent_2,
            rgb(0x233348).into(),
        ),
    };
    let fill_alpha = if config.checked { 0.92 } else { 0.28 };

    div()
        .relative()
        .group(SharedString::from(config.group))
        .flex()
        .items_center()
        .gap(px(8.))
        .flex_shrink_0()
        .child(
            div()
                .id(("jelly-switch-track", config.id_seed))
                .relative()
                .w(px(track_w))
                .h(px(track_h))
                .rounded(px(999.))
                .overflow_hidden()
                .border_1()
                .border_color(hsla(0., 0., 1., (0.56 + pulse) * opacity))
                .bg(linear_gradient(
                    135.,
                    linear_color_stop(rgb(0xe7fbff), 0.0),
                    linear_color_stop(rgb(0xffeef6), 1.0),
                ))
                .shadow(vec![
                    gpui::BoxShadow {
                        color: aura.opacity((0.18 + pulse) * opacity),
                        offset: gpui::point(px(0.), px(10.)),
                        blur_radius: px(24.),
                        spread_radius: px(-14.),
                    },
                    gpui::BoxShadow {
                        color: hsla(0., 0., 1., 0.5 * opacity),
                        offset: gpui::point(px(0.), px(1.)),
                        blur_radius: px(0.),
                        spread_radius: px(0.),
                    },
                ])
                .group_active(SharedString::from(config.group), |this| {
                    this.border_color(hsla(0., 0., 1., 0.58 * opacity))
                })
                .child(
                    div()
                        .absolute()
                        .left(px(5.))
                        .right(px(5.))
                        .top(px(5.))
                        .bottom(px(5.))
                        .rounded(px(999.))
                        .bg(hsla(211., 0.32, 0.18, 0.08 * opacity)),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(6.))
                        .right(px(6.))
                        .top(px(4.))
                        .h(px(track_h * 0.33))
                        .rounded(px(999.))
                        .bg(hsla(0., 0., 1., (0.28 + pulse) * opacity)),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(4.))
                        .top(px(4.))
                        .bottom(px(4.))
                        .w(relative(if config.checked { 0.96 } else { 0.46 }))
                        .rounded(px(999.))
                        .bg(linear_gradient(
                            90.,
                            linear_color_stop(start.opacity(fill_alpha * opacity), 0.0),
                            linear_color_stop(end.opacity((fill_alpha + 0.04) * opacity), 1.0),
                        )),
                )
                .child(
                    div()
                        .id(("jelly-switch-thumb", config.id_seed))
                        .absolute()
                        .left(px(thumb_left))
                        .top(px((track_h - thumb) * 0.5))
                        .w(px(thumb + if config.active { pulse * 12. } else { 0. }))
                        .h(px(thumb - if config.active { pulse * 5. } else { 0. }))
                        .rounded(px(999.))
                        .border_1()
                        .border_color(hsla(0., 0., 1., (0.72 + pulse) * opacity))
                        .bg(linear_gradient(
                            145.,
                            linear_color_stop(hsla(0., 0., 1., 0.98 * opacity), 0.0),
                            linear_color_stop(end.opacity(0.46 * opacity), 1.0),
                        ))
                        .shadow(vec![
                            gpui::BoxShadow {
                                color: aura.opacity((0.24 + pulse) * opacity),
                                offset: gpui::point(px(0.), px(8.)),
                                blur_radius: px(18.),
                                spread_radius: px(-8.),
                            },
                            gpui::BoxShadow {
                                color: hsla(0., 0., 1., 0.64 * opacity),
                                offset: gpui::point(px(0.), px(1.)),
                                blur_radius: px(0.),
                                spread_radius: px(0.),
                            },
                        ])
                        .group_active(SharedString::from(config.group), |this| {
                            this.top(px((track_h - thumb) * 0.5 + 3.))
                                .w(px(thumb + 5.))
                                .h(px(thumb - 5.))
                        })
                        .child(
                            div()
                                .absolute()
                                .left(px(7.))
                                .right(px(7.))
                                .top(px(5.))
                                .h(px((thumb * 0.22).max(5.)))
                                .rounded(px(999.))
                                .bg(hsla(0., 0., 1., (0.42 + pulse) * opacity)),
                        ),
                ),
        )
        .child(
            div()
                .text_size(px(text_size))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(text.opacity(if config.enabled { 1. } else { 0.62 }))
                .child(config.label),
        )
}
