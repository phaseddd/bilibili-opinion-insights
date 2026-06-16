use gpui::{
    Bounds, Corners, FontWeight, InteractiveElement as _, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, div, hsla, linear_color_stop,
    linear_gradient, point, prelude::FluentBuilder as _, px, size,
};

use crate::gui::materials::{JellyMaterialToken, JellyTone};
use crate::gui::motion::JellyMotionSnapshot;
use crate::gui::rendering::gpui_layers::{
    lower_refractive_ridge, shell_contact_shadow, top_specular_band,
};
use crate::gui::rendering::jelly_image_cache::JellyButtonImage;
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

#[derive(Clone)]
pub struct JellyButtonConfig {
    pub tone: JellyActionTone,
    pub enabled: bool,
    pub loading: bool,
    pub motion_tick: u64,
    pub group: &'static str,
    pub id_seed: usize,
    pub size: JellyButtonSize,
    pub motion: JellyMotionSnapshot,
    pub image: Option<JellyButtonImage>,
}

#[derive(Clone, Copy)]
pub enum HeaderActionKind {
    Ghost,
    Outline,
    Primary,
}

#[derive(Clone)]
pub struct HeaderButtonConfig {
    pub kind: HeaderActionKind,
    pub enabled: bool,
    pub motion_tick: u64,
    pub group: &'static str,
    pub id_seed: usize,
    pub motion: JellyMotionSnapshot,
    pub image: Option<JellyButtonImage>,
}

#[derive(Clone, Copy)]
struct JellyButtonMetrics {
    height: f32,
    min_width: f32,
    outer_pad_x: f32,
    text_size: f32,
    core_x: f32,
    core_y: f32,
    shell_top: f32,
    shell_bottom: f32,
    highlight_h: f32,
}

impl JellyButtonSize {
    fn metrics(self) -> JellyButtonMetrics {
        match self {
            Self::Standard => JellyButtonMetrics {
                height: 66.,
                min_width: 172.,
                outer_pad_x: 24.,
                text_size: 12.,
                core_x: 82.,
                core_y: 34.,
                shell_top: 0.,
                shell_bottom: 3.,
                highlight_h: 7.5,
            },
            Self::Compact => JellyButtonMetrics {
                height: 48.,
                min_width: 108.,
                outer_pad_x: 16.,
                text_size: 11.,
                core_x: 54.,
                core_y: 25.,
                shell_top: 0.,
                shell_bottom: 2.,
                highlight_h: 5.5,
            },
        }
    }
}

#[derive(Clone, Copy)]
struct JellyButtonShape {
    shell_top: f32,
    shell_bottom: f32,
    shell_bleed_x: f32,
    shell_rounding: f32,
    core_left: f32,
    core_right: f32,
    core_top: f32,
    core_bottom: f32,
    core_rounding: f32,
    label_y: f32,
    contact_offset: f32,
    contact_blur: f32,
    disabled_alpha: f32,
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
            motion: config.motion,
            image: config.image,
        },
    )
}

fn tone_token(tone: JellyActionTone, palette: &Palette) -> JellyMaterialToken {
    JellyMaterialToken::for_tone(
        match tone {
            JellyActionTone::Primary => JellyTone::Primary,
            JellyActionTone::Cyan => JellyTone::Cyan,
            JellyActionTone::Warning => JellyTone::Warning,
            JellyActionTone::Neutral => JellyTone::Neutral,
        },
        palette,
    )
}

fn button_shape(
    metrics: JellyButtonMetrics,
    material: JellyMaterialToken,
    config: &JellyButtonConfig,
) -> JellyButtonShape {
    let motion = config.motion;
    let shell_depth = material.shell_depth;
    let size_factor = if matches!(config.size, JellyButtonSize::Standard) {
        1.
    } else {
        0.72
    };
    let enabled_alpha = if config.enabled { 1. } else { 0.46 };
    let loading_breath = if config.loading {
        ((config.motion_tick as f32 * 0.18).sin().mul_add(0.5, 0.5)) * 0.1
    } else {
        0.
    };
    let pressure = (motion.pressure + loading_breath).clamp(0., 1.);
    let rebound = motion.rebound;
    let outward = shell_depth * 4.4 * size_factor
        + motion.squash_x * (15. + shell_depth * 8.) * size_factor
        + rebound.max(0.) * (10. + shell_depth * 5.2) * size_factor;
    let press_down =
        pressure * (6.4 + shell_depth * 1.4) * size_factor + motion.squash_y * 3.2 * size_factor;
    let lift = rebound.max(0.) * (3.4 + shell_depth * 1.3) * size_factor;

    JellyButtonShape {
        shell_top: metrics.shell_top + press_down - lift,
        shell_bottom: metrics.shell_bottom - pressure * 2.8 * size_factor + lift * 0.52,
        shell_bleed_x: outward,
        shell_rounding: 999.,
        core_left: metrics.core_x
            + shell_depth * 18.
            + motion.inner_lag * 6.2 * size_factor
            + pressure * 8. * size_factor,
        core_right: metrics.core_x
            + shell_depth * 18.
            + motion.inner_lag * 5.2 * size_factor
            + pressure * 7. * size_factor,
        core_top: metrics.core_y
            + shell_depth * 7.2
            + press_down * 0.5
            + motion.inner_lag * 2.6 * size_factor,
        core_bottom: metrics.core_y
            + shell_depth * 6.2
            + pressure * 1.2 * size_factor
            + motion.inner_lag * 1.2,
        core_rounding: 999.,
        label_y: press_down * 0.35 - lift * 0.22,
        contact_offset: 15. - pressure * 5. + rebound.max(0.) * 2.,
        contact_blur: 30. + motion.contact * 14.,
        disabled_alpha: enabled_alpha,
    }
}

pub fn jelly_action_button(
    label: impl Into<String>,
    palette: &Palette,
    config: JellyButtonConfig,
) -> gpui::Div {
    let label = label.into();
    let material = tone_token(config.tone, palette);
    let metrics = config.size.metrics();
    let shape = button_shape(metrics, material, &config);
    let opacity = shape.disabled_alpha;
    let motion = config.motion;
    let group_name = SharedString::from(config.group);
    let id_seed = config.id_seed;
    let shell_alpha = material.shell_alpha * opacity;
    let core_alpha = material.core_alpha * 0.48 * opacity;
    let trough_alpha = (0.3 + motion.inner_lag * 0.18 + motion.pressure * 0.14) * opacity;
    let button_image = config.image.clone();
    let has_button_image = button_image.is_some();

    div()
        .relative()
        .group(group_name.clone())
        .flex_shrink_0()
        .h(px(metrics.height))
        .min_w(px(metrics.min_width))
        .px(px(metrics.outer_pad_x))
        .rounded(px(999.))
        .child(shell_contact_shadow(material, motion, opacity))
        .child(
            div()
                .id(("jelly-button-shell", id_seed))
                .absolute()
                .top(px(shape.shell_top))
                .bottom(px(shape.shell_bottom.max(0.)))
                .left(px(-shape.shell_bleed_x))
                .right(px(-shape.shell_bleed_x))
                .rounded(px(shape.shell_rounding))
                .overflow_hidden()
                .border_1()
                .border_color(
                    material
                        .rim
                        .opacity((0.66 + motion.rim_pressure * 0.34) * opacity),
                )
                .bg(linear_gradient(
                    135.,
                    linear_color_stop(material.shell_start.opacity(shell_alpha), 0.0),
                    linear_color_stop(material.shell_end.opacity(shell_alpha), 1.0),
                ))
                .shadow(vec![
                    gpui::BoxShadow {
                        color: material
                            .state_aura
                            .opacity((0.28 + motion.aura * 0.18) * opacity),
                        offset: gpui::point(px(0.), px(shape.contact_offset)),
                        blur_radius: px(shape.contact_blur),
                        spread_radius: px(-14.),
                    },
                    gpui::BoxShadow {
                        color: material.inner_glow.opacity(0.62 * opacity),
                        offset: gpui::point(px(0.), px(7.)),
                        blur_radius: px(34.),
                        spread_radius: px(-7.),
                    },
                    gpui::BoxShadow {
                        color: material.rim.opacity(0.58 * opacity),
                        offset: gpui::point(px(0.), px(1.)),
                        blur_radius: px(0.),
                        spread_radius: px(0.),
                    },
                ])
                .when(config.enabled, |this| {
                    this.hover(|this| {
                        this.border_color(material.rim.opacity(0.96 * opacity))
                            .shadow(vec![
                                gpui::BoxShadow {
                                    color: material.state_aura.opacity(0.36 * opacity),
                                    offset: gpui::point(px(0.), px(12.)),
                                    blur_radius: px(34.),
                                    spread_radius: px(-14.),
                                },
                                gpui::BoxShadow {
                                    color: material.inner_glow.opacity(0.46 * opacity),
                                    offset: gpui::point(px(0.), px(6.)),
                                    blur_radius: px(24.),
                                    spread_radius: px(-10.),
                                },
                            ])
                    })
                })
                .group_active(group_name.clone(), |this| {
                    this.top(px(metrics.shell_top + 7.))
                        .bottom(px(1.))
                        .left(px(-9.))
                        .right(px(-9.))
                        .border_color(material.rim.opacity(0.58 * opacity))
                })
                .when(has_button_image, move |this| {
                    let Some(image) = button_image.clone() else {
                        return this;
                    };
                    this.child(
                        canvas(
                            move |_, _window: &mut Window, _cx| (),
                            move |bounds, _, window: &mut Window, _cx| {
                                let origin_x = f32::from(bounds.origin.x);
                                let origin_y = f32::from(bounds.origin.y);
                                let track_w = f32::from(bounds.size.width);
                                let track_h = f32::from(bounds.size.height);
                                let image_bounds = Bounds::new(
                                    point(px(origin_x), px(origin_y)),
                                    size(px(track_w), px(track_h)),
                                );
                                let _ = window.paint_image(
                                    image_bounds,
                                    Corners::from(px(track_h * 0.5)),
                                    image.image.clone(),
                                    0,
                                    false,
                                );
                            },
                        )
                        .absolute()
                        .inset_0(),
                    )
                })
                .when(!has_button_image, |this| {
                    this.child(
                        div()
                            .absolute()
                            .left(px(5. - motion.squash_x * 5.))
                            .right(px(5. - motion.squash_x * 4.))
                            .top(px(5. + motion.pressure * 1.4))
                            .bottom(px(3. - motion.pressure * 0.9))
                            .rounded(px(999.))
                            .bg(material
                                .shell_mid
                                .opacity((0.46 + motion.gloss_phase * 0.12) * opacity)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(2. - motion.squash_x * 5.))
                            .right(px(2. - motion.squash_x * 4.))
                            .bottom(px(1.))
                            .h(px(metrics.height * 0.56))
                            .rounded(px(999.))
                            .bg(material.contact_shadow.opacity(0.3 * opacity)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(5.))
                            .right(px(5.))
                            .top(px(3. + motion.pressure * 0.8))
                            .h(px(metrics.height * 0.3))
                            .rounded(px(999.))
                            .bg(linear_gradient(
                                180.,
                                linear_color_stop(material.specular.opacity(0.3 * opacity), 0.0),
                                linear_color_stop(material.shell_mid.opacity(0.18 * opacity), 1.0),
                            )),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(shape.core_left - 12.))
                            .right(px(shape.core_right - 12.))
                            .top(px((shape.core_top - 7.).max(4.)))
                            .bottom(px((shape.core_bottom - 8.).max(4.)))
                            .rounded(px(999.))
                            .border_1()
                            .border_color(
                                material
                                    .rim
                                    .opacity((0.2 + motion.rim_pressure * 0.12) * opacity),
                            )
                            .bg(linear_gradient(
                                180.,
                                linear_color_stop(
                                    material.contact_shadow.opacity(trough_alpha),
                                    0.0,
                                ),
                                linear_color_stop(
                                    material
                                        .shell_mid
                                        .opacity((0.32 + motion.aura * 0.12) * opacity),
                                    1.0,
                                ),
                            ))
                            .shadow(vec![
                                gpui::BoxShadow {
                                    color: material.contact_shadow.opacity(0.24 * opacity),
                                    offset: gpui::point(px(0.), px(4.)),
                                    blur_radius: px(12.),
                                    spread_radius: px(-8.),
                                },
                                gpui::BoxShadow {
                                    color: material.specular.opacity(0.12 * opacity),
                                    offset: gpui::point(px(0.), px(-1.)),
                                    blur_radius: px(0.),
                                    spread_radius: px(0.),
                                },
                            ]),
                    )
                })
                .when(!has_button_image, |this| {
                    this.when(
                        shape.core_left + shape.core_right < metrics.min_width - 28.,
                        |this| {
                            this.child(
                                div()
                                    .absolute()
                                    .left(px(shape.core_left + 5.))
                                    .right(px(shape.core_right + 5.))
                                    .top(px((shape.core_top + 3.).max(6.)))
                                    .bottom(px((shape.core_bottom + 4.).max(6.)))
                                    .rounded(px(shape.core_rounding))
                                    .border_1()
                                    .border_color(
                                        material
                                            .rim
                                            .opacity((0.12 + motion.rim_pressure * 0.08) * opacity),
                                    )
                                    .bg(linear_gradient(
                                        180.,
                                        linear_color_stop(
                                            material.core_top.opacity(core_alpha),
                                            0.0,
                                        ),
                                        linear_color_stop(
                                            material.core_bottom.opacity(core_alpha),
                                            1.0,
                                        ),
                                    ))
                                    .shadow(vec![
                                        gpui::BoxShadow {
                                            color: hsla(0., 0., 1., 0.12 * opacity),
                                            offset: gpui::point(px(0.), px(1.)),
                                            blur_radius: px(0.),
                                            spread_radius: px(0.),
                                        },
                                        gpui::BoxShadow {
                                            color: material.state_aura.opacity(0.16 * opacity),
                                            offset: gpui::point(px(0.), px(8.)),
                                            blur_radius: px(20.),
                                            spread_radius: px(-7.),
                                        },
                                    ]),
                            )
                        },
                    )
                    .child(top_specular_band(
                        material,
                        motion,
                        if matches!(config.size, JellyButtonSize::Standard) {
                            52.
                        } else {
                            32.
                        },
                        metrics.highlight_h * 0.82,
                        opacity,
                    ))
                    .child(
                        div()
                            .absolute()
                            .left(px(46. - motion.squash_x * 6.))
                            .right(px(62. - motion.squash_x * 4.))
                            .top(px(4. + motion.pressure * 1.8))
                            .h(px(1.4))
                            .rounded(px(999.))
                            .bg(material
                                .rim
                                .opacity((0.24 + motion.rim_pressure * 0.14) * opacity)),
                    )
                    .child(lower_refractive_ridge(material, motion, opacity))
                    .child(
                        div()
                            .absolute()
                            .left(px(12.))
                            .right(px(12.))
                            .bottom(px(0.))
                            .h(px(1.))
                            .bg(material
                                .rim
                                .opacity((0.26 + motion.rim_pressure * 0.2) * opacity)),
                    )
                }),
        )
        .child(
            div()
                .id(("jelly-button-label", id_seed))
                .absolute()
                .left(px(metrics.outer_pad_x * 0.5 + motion.error_shake * 7.))
                .right(px(metrics.outer_pad_x * 0.5 - motion.error_shake * 7.))
                .top(px(shape.label_y))
                .bottom(px(0.))
                .flex()
                .items_center()
                .justify_center()
                .group_active(group_name, |this| {
                    this.pt(px(if matches!(config.size, JellyButtonSize::Standard) {
                        3.
                    } else {
                        2.
                    }))
                })
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(
                            material
                                .text
                                .opacity(if config.enabled { 1.0 } else { 0.58 }),
                        )
                        .text_size(px(metrics.text_size))
                        .child(SharedString::from(label)),
                ),
        )
}
