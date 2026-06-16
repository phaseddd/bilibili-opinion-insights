use gpui::{Div, Styled as _, div, hsla, linear_color_stop, linear_gradient, px};

use crate::gui::materials::JellyMaterialToken;
use crate::gui::motion::JellyMotionSnapshot;

pub(crate) fn shell_contact_shadow(
    material: JellyMaterialToken,
    motion: JellyMotionSnapshot,
    opacity: f32,
) -> Div {
    div()
        .absolute()
        .left(px(4. - motion.squash_x * 9.))
        .right(px(4. - motion.squash_x * 9.))
        .bottom(px(0.))
        .h(px(14. + motion.contact * 7.))
        .rounded(px(999.))
        .bg(material
            .contact_shadow
            .opacity((0.2 + motion.contact * 0.26 + motion.aura * 0.1) * opacity))
}

pub(crate) fn top_specular_band(
    material: JellyMaterialToken,
    motion: JellyMotionSnapshot,
    inset: f32,
    height: f32,
    opacity: f32,
) -> Div {
    div()
        .absolute()
        .left(px(inset - motion.squash_x * 5.))
        .right(px(inset + motion.squash_x * 3.))
        .top(px(
            4.8 + motion.pressure * 2.8 - motion.rebound.max(0.) * 1.9
        ))
        .h(px((height - motion.pressure * 2.2).max(2.8)))
        .rounded(px(999.))
        .bg(linear_gradient(
            90.,
            linear_color_stop(material.specular.opacity(0.12 * opacity), 0.0),
            linear_color_stop(
                material
                    .specular
                    .opacity((0.36 + motion.gloss_phase * 0.32) * opacity),
                1.0,
            ),
        ))
}

pub(crate) fn lower_refractive_ridge(
    material: JellyMaterialToken,
    motion: JellyMotionSnapshot,
    opacity: f32,
) -> Div {
    div()
        .absolute()
        .left(px(5. - motion.squash_x * 7.))
        .right(px(5. - motion.squash_x * 6.))
        .bottom(px(4. - motion.pressure * 2.))
        .h(px(12. + motion.rebound.max(0.) * 4.2))
        .rounded(px(999.))
        .bg(linear_gradient(
            90.,
            linear_color_stop(hsla(0., 0., 1., 0.08 * opacity), 0.0),
            linear_color_stop(
                material
                    .specular
                    .opacity((0.32 + motion.rim_pressure * 0.28) * opacity),
                1.0,
            ),
        ))
}
