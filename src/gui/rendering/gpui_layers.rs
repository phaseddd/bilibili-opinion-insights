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
        .left(px(7. - motion.squash_x * 7.))
        .right(px(7. - motion.squash_x * 7.))
        .bottom(px(0.))
        .h(px(12. + motion.contact * 5.))
        .rounded(px(999.))
        .bg(material
            .contact_shadow
            .opacity((0.16 + motion.contact * 0.22 + motion.aura * 0.08) * opacity))
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
        .left(px(inset - motion.squash_x * 4.))
        .right(px(inset + motion.squash_x * 2.))
        .top(px(4. + motion.pressure * 2.4 - motion.rebound.max(0.) * 1.6))
        .h(px((height - motion.pressure * 1.7).max(3.)))
        .rounded(px(999.))
        .bg(linear_gradient(
            90.,
            linear_color_stop(material.specular.opacity(0.18 * opacity), 0.0),
            linear_color_stop(
                material
                    .specular
                    .opacity((0.42 + motion.gloss_phase * 0.38) * opacity),
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
        .left(px(9. - motion.squash_x * 5.))
        .right(px(9. - motion.squash_x * 4.))
        .bottom(px(5. - motion.pressure * 1.7))
        .h(px(9. + motion.rebound.max(0.) * 3.4))
        .rounded(px(999.))
        .bg(linear_gradient(
            90.,
            linear_color_stop(hsla(0., 0., 1., 0.07 * opacity), 0.0),
            linear_color_stop(
                material
                    .specular
                    .opacity((0.28 + motion.rim_pressure * 0.24) * opacity),
                1.0,
            ),
        ))
}
