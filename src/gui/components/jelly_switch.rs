use gpui::{
    Bounds, Corners, FontWeight, InteractiveElement as _, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, div, hsla, linear_color_stop,
    linear_gradient, point, prelude::FluentBuilder as _, px, size,
};

use crate::gui::materials::{JellyMaterialToken, JellyTone};
use crate::gui::motion::JellyMotionSnapshot;
use crate::gui::rendering::gpui_layers::{lower_refractive_ridge, top_specular_band};
use crate::gui::rendering::jelly_image_cache::JellySwitchImage;
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

#[derive(Clone)]
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
    pub image: Option<JellySwitchImage>,
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
    let group_name = SharedString::from(config.group);
    let switch_image = config.image.clone();
    let has_switch_image = switch_image.is_some();
    let track_alpha = if has_switch_image { 0.22 } else { 1. };
    let track_shadow_scale = if has_switch_image { 0.45 } else { 1. };

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
                    linear_color_stop(hsla(190., 0.86, 0.97, 0.88 * opacity * track_alpha), 0.0),
                    linear_color_stop(hsla(332., 0.82, 0.97, 0.86 * opacity * track_alpha), 1.0),
                ))
                .shadow(vec![
                    gpui::BoxShadow {
                        color: material.state_aura.opacity(
                            (0.14 + active_wave + config.motion.aura * 0.18)
                                * opacity
                                * track_shadow_scale,
                        ),
                        offset: gpui::point(px(0.), px(10.)),
                        blur_radius: px(25.),
                        spread_radius: px(-13.),
                    },
                    gpui::BoxShadow {
                        color: material
                            .inner_glow
                            .opacity(0.32 * opacity * track_shadow_scale),
                        offset: gpui::point(px(0.), px(1.)),
                        blur_radius: px(0.),
                        spread_radius: px(0.),
                    },
                ])
                .group_active(group_name.clone(), |this| {
                    this.border_color(material.rim.opacity(0.56 * opacity))
                })
                .when(has_switch_image, move |this| {
                    let Some(image) = switch_image.clone() else {
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
                .when(!has_switch_image, |this| {
                    this.child(
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
                                linear_color_stop(material.shell_mid.opacity(0.78 * opacity), 0.0),
                                linear_color_stop(material.shell_end.opacity(0.66 * opacity), 1.0),
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
                                    color: material.inner_glow.opacity(0.36 * opacity),
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
                                12.,
                                (thumb * 0.14).max(4.),
                                opacity,
                            ))
                            .child(
                                div()
                                    .absolute()
                                    .right(px(7.))
                                    .bottom(px(7.))
                                    .size(px((thumb * 0.28).max(8.)))
                                    .rounded(px(999.))
                                    .bg(material.specular.opacity(0.24 * opacity)),
                            )
                            .child(lower_refractive_ridge(material, config.motion, opacity)),
                    )
                }),
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
