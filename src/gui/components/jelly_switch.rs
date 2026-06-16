use gpui::{
    FontWeight, InteractiveElement as _, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, hsla, linear_color_stop, linear_gradient,
    px, relative,
};

use crate::gui::materials::{JellyMaterialToken, JellyTone};
use crate::gui::motion::JellyMotionSnapshot;
use crate::gui::rendering::gpui_layers::{lower_refractive_ridge, top_specular_band};
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
    pub motion: JellyMotionSnapshot,
}

pub fn jelly_switch(config: JellySwitchConfig, palette: &Palette) -> gpui::Div {
    let (track_w, track_h, thumb, text_size) = match config.size {
        JellySwitchSize::Standard => (142., 52., 43., 12.),
        JellySwitchSize::Compact => (100., 36., 30., 11.),
    };
    let material = switch_material(config.tone, palette);
    let opacity = if config.enabled { 1. } else { 0.46 };
    let progress = if config.checked { 1. } else { 0. };
    let endpoint = if config.checked { 1. } else { -1. };
    let active_wave = if config.active {
        ((config.motion_tick as f32 * 0.34).sin().mul_add(0.5, 0.5)) * 0.18
    } else {
        0.
    };
    let settle = config.motion.rebound;
    let wiggle = (settle * 5.5 + config.motion.error_shake * 2.).clamp(-5.5, 5.5);
    let travel = track_w - thumb - 10.;
    let thumb_left =
        5. + travel * progress + endpoint as f32 * (config.motion.squash_x * 4. + settle * 3.);
    let thumb_w = thumb + config.motion.squash_x * 9. + active_wave * 8.;
    let thumb_h = thumb - config.motion.squash_y * 6. - active_wave * 3.;
    let fill_alpha = if config.checked { 0.88 } else { 0.18 };
    let group_name = SharedString::from(config.group);

    div()
        .relative()
        .group(group_name.clone())
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
                .border_color(
                    material.rim.opacity(
                        (0.45 + active_wave + config.motion.rim_pressure * 0.24) * opacity,
                    ),
                )
                .bg(linear_gradient(
                    135.,
                    linear_color_stop(hsla(190., 0.86, 0.97, 0.88 * opacity), 0.0),
                    linear_color_stop(hsla(332., 0.82, 0.97, 0.86 * opacity), 1.0),
                ))
                .shadow(vec![
                    gpui::BoxShadow {
                        color: material
                            .state_aura
                            .opacity((0.14 + active_wave + config.motion.aura * 0.18) * opacity),
                        offset: gpui::point(px(0.), px(10.)),
                        blur_radius: px(25.),
                        spread_radius: px(-13.),
                    },
                    gpui::BoxShadow {
                        color: hsla(0., 0., 1., 0.46 * opacity),
                        offset: gpui::point(px(0.), px(1.)),
                        blur_radius: px(0.),
                        spread_radius: px(0.),
                    },
                ])
                .group_active(group_name.clone(), |this| {
                    this.border_color(material.rim.opacity(0.56 * opacity))
                })
                .child(
                    div()
                        .absolute()
                        .left(px(6.))
                        .right(px(6.))
                        .top(px(6.))
                        .bottom(px(6.))
                        .rounded(px(999.))
                        .bg(hsla(210., 0.3, 0.18, 0.09 * opacity)),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(5.))
                        .top(px(5.))
                        .bottom(px(5.))
                        .w(relative(if config.checked { 0.96 } else { 0.42 }))
                        .rounded(px(999.))
                        .bg(linear_gradient(
                            90.,
                            linear_color_stop(
                                material.shell_start.opacity(fill_alpha * opacity),
                                0.0,
                            ),
                            linear_color_stop(
                                material.shell_end.opacity((fill_alpha + 0.02) * opacity),
                                1.0,
                            ),
                        )),
                )
                .child(
                    div()
                        .id(("jelly-switch-thumb", config.id_seed))
                        .absolute()
                        .left(px(thumb_left + wiggle))
                        .top(px((track_h - thumb_h) * 0.5 + config.motion.pressure * 2.2))
                        .w(px(thumb_w))
                        .h(px(thumb_h.max(24.)))
                        .rounded(px(999.))
                        .border_1()
                        .border_color(
                            material
                                .rim
                                .opacity((0.64 + config.motion.rim_pressure * 0.24) * opacity),
                        )
                        .bg(linear_gradient(
                            145.,
                            linear_color_stop(material.core_top.opacity(0.95 * opacity), 0.0),
                            linear_color_stop(material.shell_end.opacity(0.42 * opacity), 1.0),
                        ))
                        .shadow(vec![
                            gpui::BoxShadow {
                                color: material.state_aura.opacity(
                                    (0.2 + active_wave + config.motion.aura * 0.2) * opacity,
                                ),
                                offset: gpui::point(px(0.), px(8.)),
                                blur_radius: px(18.),
                                spread_radius: px(-8.),
                            },
                            gpui::BoxShadow {
                                color: hsla(0., 0., 1., 0.58 * opacity),
                                offset: gpui::point(px(0.), px(1.)),
                                blur_radius: px(0.),
                                spread_radius: px(0.),
                            },
                        ])
                        .group_active(group_name.clone(), |this| {
                            this.top(px((track_h - thumb) * 0.5 + 4.))
                                .w(px(thumb + 7.))
                                .h(px(thumb - 6.))
                        })
                        .child(top_specular_band(
                            material,
                            config.motion,
                            8.,
                            (thumb * 0.2).max(5.),
                            opacity,
                        ))
                        .child(lower_refractive_ridge(material, config.motion, opacity)),
                ),
        )
        .child(
            div()
                .text_size(px(text_size))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(
                    material
                        .text
                        .opacity(if config.enabled { 1. } else { 0.58 }),
                )
                .child(config.label),
        )
}

fn switch_material(tone: JellySwitchTone, palette: &Palette) -> JellyMaterialToken {
    JellyMaterialToken::for_tone(
        match tone {
            JellySwitchTone::Primary => JellyTone::Primary,
            JellySwitchTone::Cyan => JellyTone::Cyan,
            JellySwitchTone::Output => JellyTone::Output,
        },
        palette,
    )
}
