use gpui::{
    FontWeight, Hsla, InteractiveElement as _, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, hsla, linear_color_stop, linear_gradient,
    prelude::FluentBuilder as _, px, rgb,
};

use crate::gui::motion::wave_between;
use crate::gui::theme::Palette;

#[derive(Clone, Copy)]
pub enum JellyActionTone {
    Primary,
    Cyan,
    Warning,
    Neutral,
}

#[derive(Clone, Copy)]
pub enum JellyButtonSize {
    Standard,
    Compact,
}

#[derive(Clone, Copy)]
pub struct JellyButtonConfig {
    pub tone: JellyActionTone,
    pub enabled: bool,
    pub loading: bool,
    pub motion_tick: u64,
    pub group: &'static str,
    pub id_seed: usize,
    pub size: JellyButtonSize,
    pub rebound: f32,
}

#[derive(Clone, Copy)]
pub enum HeaderActionKind {
    Ghost,
    Outline,
    Primary,
}

#[derive(Clone, Copy)]
pub struct HeaderButtonConfig {
    pub kind: HeaderActionKind,
    pub enabled: bool,
    pub motion_tick: u64,
    pub group: &'static str,
    pub id_seed: usize,
    pub rebound: f32,
}

#[derive(Clone, Copy)]
struct JellyButtonMetrics {
    height: f32,
    min_width: f32,
    outer_pad_x: f32,
    text_size: f32,
    inner_x: f32,
    inner_y: f32,
    highlight_h: f32,
    shell_top: f32,
    shell_bottom: f32,
}

impl JellyButtonSize {
    fn metrics(self) -> JellyButtonMetrics {
        match self {
            Self::Standard => JellyButtonMetrics {
                height: 56.,
                min_width: 152.,
                outer_pad_x: 20.,
                text_size: 12.,
                inner_x: 17.,
                inner_y: 10.,
                highlight_h: 8.,
                shell_top: 1.5,
                shell_bottom: 4.5,
            },
            Self::Compact => JellyButtonMetrics {
                height: 42.,
                min_width: 92.,
                outer_pad_x: 13.,
                text_size: 11.,
                inner_x: 12.,
                inner_y: 7.,
                highlight_h: 5.5,
                shell_top: 1.2,
                shell_bottom: 3.4,
            },
        }
    }
}

#[derive(Clone, Copy)]
struct JellyButtonMaterial {
    start: Hsla,
    end: Hsla,
    rim: Hsla,
    core_top: Hsla,
    core_bottom: Hsla,
    text: Hsla,
    aura: Hsla,
}

#[derive(Clone, Copy)]
struct JellyButtonMotion {
    shell_top: f32,
    shell_bottom: f32,
    shell_bleed_x: f32,
    core_top: f32,
    core_bottom: f32,
    core_bleed_x: f32,
    gloss_top: f32,
    gloss_inset: f32,
    rim_gloss_top: f32,
    ridge_bottom: f32,
    ridge_height: f32,
    contact_y: f32,
    contact_blur: f32,
    breath: f32,
    pop: f32,
}

pub fn header_action_button(
    label: &'static str,
    palette: &Palette,
    config: HeaderButtonConfig,
) -> gpui::Div {
    let tone = match config.kind {
        HeaderActionKind::Ghost => JellyActionTone::Neutral,
        HeaderActionKind::Outline => JellyActionTone::Warning,
        HeaderActionKind::Primary => JellyActionTone::Primary,
    };

    jelly_action_button(
        label,
        palette,
        JellyButtonConfig {
            tone,
            enabled: config.enabled,
            loading: false,
            motion_tick: config.motion_tick,
            group: config.group,
            id_seed: config.id_seed,
            size: JellyButtonSize::Compact,
            rebound: config.rebound,
        },
    )
}

fn jelly_button_material(tone: JellyActionTone, palette: &Palette) -> JellyButtonMaterial {
    match tone {
        JellyActionTone::Primary => JellyButtonMaterial {
            start: palette.accent_2,
            end: palette.accent,
            rim: hsla(0., 0., 1., 0.72),
            core_top: hsla(0., 0., 1., 0.98),
            core_bottom: hsla(190., 0.84, 0.92, 0.82),
            text: rgb(0x075b70).into(),
            aura: palette.accent,
        },
        JellyActionTone::Cyan => JellyButtonMaterial {
            start: palette.accent,
            end: rgb(0x00a7d8).into(),
            rim: hsla(0., 0., 1., 0.74),
            core_top: hsla(0., 0., 1., 0.97),
            core_bottom: hsla(188., 0.86, 0.91, 0.8),
            text: rgb(0x075c67).into(),
            aura: palette.accent,
        },
        JellyActionTone::Warning => JellyButtonMaterial {
            start: rgb(0xffa83d).into(),
            end: palette.warning,
            rim: hsla(0., 0., 1., 0.64),
            core_top: hsla(0., 0., 1., 0.96),
            core_bottom: hsla(35., 0.92, 0.88, 0.78),
            text: rgb(0x884900).into(),
            aura: palette.warning,
        },
        JellyActionTone::Neutral => JellyButtonMaterial {
            start: rgb(0xb7f4ff).into(),
            end: rgb(0xffd7e8).into(),
            rim: palette.accent.opacity(0.5),
            core_top: hsla(0., 0., 1., 0.96),
            core_bottom: hsla(332., 0.75, 0.95, 0.72),
            text: rgb(0x233348).into(),
            aura: palette.accent,
        },
    }
}

fn jelly_button_motion(
    metrics: JellyButtonMetrics,
    config: JellyButtonConfig,
) -> JellyButtonMotion {
    let pop = config.rebound.clamp(0., 1.);
    let breath = if config.loading {
        wave_between(config.motion_tick, 0.18, 0.08, 0.22)
    } else {
        0.
    };
    let size_factor = if matches!(config.size, JellyButtonSize::Standard) {
        1.
    } else {
        0.68
    };

    JellyButtonMotion {
        shell_top: metrics.shell_top - pop * 2.8 * size_factor,
        shell_bottom: metrics.shell_bottom - pop * 1.5 * size_factor,
        shell_bleed_x: pop * 6.2 * size_factor,
        core_top: metrics.inner_y - pop * 0.8 * size_factor,
        core_bottom: metrics.inner_y + pop * 2.1 * size_factor,
        core_bleed_x: pop * 1.7 * size_factor,
        gloss_top: 6. + pop * 1.3 * size_factor,
        gloss_inset: 26. - pop * 5.2 * size_factor,
        rim_gloss_top: 2.5 - pop * 0.7 * size_factor,
        ridge_bottom: 7. - pop * 1.4 * size_factor,
        ridge_height: if matches!(config.size, JellyButtonSize::Standard) {
            6.5 + pop * 2.2
        } else {
            4. + pop * 1.4
        },
        contact_y: 15. - pop * 4.2 * size_factor,
        contact_blur: 30. + pop * 10. * size_factor,
        breath,
        pop,
    }
}

pub fn jelly_action_button(
    label: impl Into<String>,
    palette: &Palette,
    config: JellyButtonConfig,
) -> gpui::Div {
    let label = label.into();
    let material = jelly_button_material(config.tone, palette);
    let opacity = if config.enabled { 1. } else { 0.46 };
    let metrics = config.size.metrics();
    let motion = jelly_button_motion(metrics, config);
    let group_name = SharedString::from(config.group);
    let id_seed = config.id_seed;

    div()
        .relative()
        .group(group_name.clone())
        .flex_shrink_0()
        .h(px(metrics.height))
        .min_w(px(metrics.min_width))
        .px(px(metrics.outer_pad_x))
        .rounded(px(999.))
        .child(
            div()
                .id(("jelly-button-shell", id_seed))
                .absolute()
                .top(px(motion.shell_top))
                .bottom(px(motion.shell_bottom))
                .left(px(-motion.shell_bleed_x))
                .right(px(-motion.shell_bleed_x))
                .rounded(px(999.))
                .overflow_hidden()
                .border_1()
                .border_color(material.rim.opacity((0.82 + motion.pop * 0.18) * opacity))
                .bg(linear_gradient(
                    135.,
                    linear_color_stop(material.start.opacity(0.9 * opacity), 0.0),
                    linear_color_stop(material.end.opacity(opacity), 1.0),
                ))
                .shadow(vec![
                    gpui::BoxShadow {
                        color: material.aura.opacity((0.26 + motion.pop * 0.16) * opacity),
                        offset: gpui::point(px(0.), px(motion.contact_y)),
                        blur_radius: px(motion.contact_blur),
                        spread_radius: px(-14.),
                    },
                    gpui::BoxShadow {
                        color: hsla(196., 0.9, 0.44, 0.20 * opacity),
                        offset: gpui::point(px(0.), px(7.)),
                        blur_radius: px(18.),
                        spread_radius: px(-9.),
                    },
                    gpui::BoxShadow {
                        color: hsla(0., 0., 1., 0.54 * opacity),
                        offset: gpui::point(px(0.), px(1.)),
                        blur_radius: px(0.),
                        spread_radius: px(0.),
                    },
                ])
                .when(config.enabled, |this| {
                    this.hover(|this| {
                        this.border_color(material.rim.opacity((0.98 * opacity).min(1.0)))
                            .bg(linear_gradient(
                                135.,
                                linear_color_stop(
                                    material.start.opacity((opacity + 0.08).min(1.0)),
                                    0.0,
                                ),
                                linear_color_stop(
                                    material.end.opacity((opacity + 0.1).min(1.0)),
                                    1.0,
                                ),
                            ))
                    })
                })
                .group_active(group_name.clone(), |this| {
                    let active_top = if matches!(config.size, JellyButtonSize::Standard) {
                        7.
                    } else {
                        4.8
                    };
                    let active_bottom = if matches!(config.size, JellyButtonSize::Standard) {
                        1.2
                    } else {
                        0.8
                    };
                    let active_bleed = if matches!(config.size, JellyButtonSize::Standard) {
                        -7.
                    } else {
                        -4.
                    };

                    this.top(px(active_top))
                        .bottom(px(active_bottom))
                        .left(px(active_bleed))
                        .right(px(active_bleed))
                        .border_color(material.rim.opacity(0.68 * opacity))
                        .shadow(vec![gpui::BoxShadow {
                            color: material.aura.opacity(0.2 * opacity),
                            offset: gpui::point(px(0.), px(7.)),
                            blur_radius: px(17.),
                            spread_radius: px(-10.),
                        }])
                })
                .child(
                    div()
                        .absolute()
                        .left(px(10.))
                        .right(px(10.))
                        .bottom(px(1.5))
                        .h(px(metrics.height * 0.30))
                        .rounded(px(999.))
                        .bg(hsla(210., 0.24, 0.08, 0.12 * opacity)),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(7.))
                        .right(px(7.))
                        .top(px(5.))
                        .h(px((metrics.height * 0.34).max(10.)))
                        .rounded(px(999.))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(hsla(0., 0., 1., 0.56 * opacity), 0.0),
                            linear_color_stop(hsla(0., 0., 1., 0.07 * opacity), 1.0),
                        )),
                )
                .child(
                    div()
                        .id(("jelly-button-core", id_seed))
                        .absolute()
                        .left(px(metrics.inner_x - motion.core_bleed_x))
                        .right(px(metrics.inner_x - motion.core_bleed_x))
                        .top(px(motion.core_top.max(3.)))
                        .bottom(px(motion.core_bottom.max(3.)))
                        .rounded(px(999.))
                        .border_1()
                        .border_color(hsla(
                            0.,
                            0.,
                            1.,
                            (0.66 + motion.breath + motion.pop * 0.22) * opacity,
                        ))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(
                                material.core_top.opacity(
                                    (0.98 + motion.breath * 0.15 + motion.pop * 0.04) * opacity,
                                ),
                                0.0,
                            ),
                            linear_color_stop(
                                material
                                    .core_bottom
                                    .opacity((0.86 + motion.pop * 0.08) * opacity),
                                1.0,
                            ),
                        ))
                        .shadow(vec![
                            gpui::BoxShadow {
                                color: hsla(0., 0., 1., (0.66 + motion.pop * 0.18) * opacity),
                                offset: gpui::point(px(0.), px(1.)),
                                blur_radius: px(0.),
                                spread_radius: px(0.),
                            },
                            gpui::BoxShadow {
                                color: material.start.opacity(0.18 * opacity),
                                offset: gpui::point(px(0.), px(8.)),
                                blur_radius: px(16.),
                                spread_radius: px(-10.),
                            },
                        ])
                        .group_active(group_name.clone(), |this| {
                            this.top(px(metrics.inner_y + 3.5))
                                .bottom(px((metrics.inner_y - 2.4).max(2.)))
                                .border_color(material.rim.opacity(0.52))
                                .bg(linear_gradient(
                                    180.,
                                    linear_color_stop(
                                        material.core_top.opacity(0.78 * opacity),
                                        0.0,
                                    ),
                                    linear_color_stop(
                                        material.core_bottom.opacity(0.68 * opacity),
                                        1.0,
                                    ),
                                ))
                        }),
                )
                .child(
                    div()
                        .id(("jelly-button-main-gloss", id_seed))
                        .absolute()
                        .top(px(motion.gloss_top))
                        .left(px(motion.gloss_inset))
                        .right(px(motion.gloss_inset))
                        .h(px(metrics.highlight_h))
                        .rounded(px(999.))
                        .bg(hsla(
                            0.,
                            0.,
                            1.,
                            (0.38 + motion.breath * 0.74 + motion.pop * 0.22) * opacity,
                        ))
                        .group_active(group_name.clone(), |this| {
                            this.top(px(8.))
                                .left(px(31.))
                                .right(px(31.))
                                .h(px((metrics.highlight_h - 1.5).max(3.)))
                                .bg(hsla(0., 0., 1., 0.24 * opacity))
                        }),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(15.))
                        .right(px(19.))
                        .top(px(motion.rim_gloss_top))
                        .h(px(2.))
                        .rounded(px(999.))
                        .bg(hsla(0., 0., 1., (0.42 + motion.pop * 0.18) * opacity)),
                )
                .child(
                    div()
                        .id(("jelly-button-ridge", id_seed))
                        .absolute()
                        .left(px(14.))
                        .right(px(14.))
                        .bottom(px(motion.ridge_bottom))
                        .h(px(motion.ridge_height))
                        .rounded(px(999.))
                        .bg(linear_gradient(
                            90.,
                            linear_color_stop(
                                hsla(0., 0., 1., (0.06 + motion.breath * 0.22) * opacity),
                                0.0,
                            ),
                            linear_color_stop(
                                hsla(0., 0., 1., (0.28 + motion.pop * 0.2) * opacity),
                                1.0,
                            ),
                        ))
                        .group_active(group_name.clone(), |this| {
                            this.bottom(px(4.))
                                .h(px(if matches!(config.size, JellyButtonSize::Standard) {
                                    4.
                                } else {
                                    3.
                                }))
                                .bg(hsla(0., 0., 1., 0.1 * opacity))
                        }),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(18.))
                        .right(px(18.))
                        .bottom(px(0.))
                        .h(px(1.))
                        .bg(material.rim.opacity((0.34 + motion.pop * 0.24) * opacity)),
                ),
        )
        .child(
            div()
                .absolute()
                .left(px(12.))
                .right(px(12.))
                .bottom(px(1.))
                .h(px(9.))
                .rounded(px(999.))
                .bg(material.aura.opacity((0.055 + motion.pop * 0.06) * opacity)),
        )
        .child(
            div()
                .absolute()
                .left(px(0.))
                .right(px(0.))
                .bottom(px(0.))
                .h(px(2.))
                .rounded(px(999.))
                .bg(hsla(0., 0., 0., 0.06 * opacity)),
        )
        .child(
            div()
                .id(("jelly-button-label", id_seed))
                .absolute()
                .left(px(metrics.outer_pad_x * 0.5))
                .right(px(metrics.outer_pad_x * 0.5))
                .top(px(0.))
                .bottom(px(0.))
                .flex()
                .items_center()
                .justify_center()
                .group_active(SharedString::from(config.group), |this| {
                    this.pt(px(if matches!(config.size, JellyButtonSize::Standard) {
                        2.
                    } else {
                        1.
                    }))
                })
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(
                            material
                                .text
                                .opacity(if config.enabled { 1.0 } else { 0.62 }),
                        )
                        .text_size(px(metrics.text_size))
                        .child(SharedString::from(label)),
                ),
        )
}
