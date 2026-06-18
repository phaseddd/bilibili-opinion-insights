use std::time::SystemTime;

use gpui::{
    Bounds, Corners, FontWeight, IntoElement, ParentElement, SharedString, Styled as _, Window,
    canvas, div, hsla, linear_color_stop, linear_gradient, point, prelude::FluentBuilder as _, px,
    relative, rgb, size,
};
use gpui_component::{h_flex, v_flex};

use crate::gui::motion::{wave_01, wave_between};
use crate::gui::rendering::jelly_image_cache::JellyCapsuleImage;
use crate::gui::state::auth::{AuthPhase, AuthState, QrState, SessionKind, SessionMode};
use crate::gui::state::events::EventKind;
use crate::gui::theme::Palette;
use crate::gui::views::primitives::{event_color, status_badge, status_dot};

pub(crate) fn glass_auth_panel(palette: &Palette) -> gpui::Div {
    v_flex()
        .p(px(18.))
        .rounded(px(24.))
        .border_1()
        .border_color(hsla(187., 0.76, 0.62, 0.22))
        .bg(hsla(0., 0., 1., 0.72))
        .shadow(vec![
            gpui::BoxShadow {
                color: palette.accent.opacity(0.16),
                offset: gpui::point(px(0.), px(18.)),
                blur_radius: px(36.),
                spread_radius: px(-18.),
            },
            gpui::BoxShadow {
                color: palette.accent_2.opacity(0.1),
                offset: gpui::point(px(0.), px(8.)),
                blur_radius: px(18.),
                spread_radius: px(-12.),
            },
        ])
}

pub(crate) fn auth_summary_block(auth: &AuthState, palette: &Palette) -> impl IntoElement {
    let message = auth
        .message
        .clone()
        .unwrap_or_else(|| "等待登录态校验。".to_string());
    let detail = auth.session.detail();

    v_flex()
        .gap(px(10.))
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(
                    v_flex()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(auth.session.title()),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(palette.muted)
                                .child(detail),
                        ),
                )
                .child(status_badge(
                    auth.phase.label(),
                    auth.status_kind(),
                    palette,
                )),
        )
        .child(
            div()
                .text_size(px(12.))
                .line_height(relative(1.35))
                .text_color(if auth.nav_error.is_some() {
                    palette.error
                } else {
                    palette.text
                })
                .child(SharedString::from(message)),
        )
}

pub(crate) fn auth_risk_block(session: &SessionMode, palette: &Palette) -> impl IntoElement {
    let (kind, title, body) = match session.kind {
        SessionKind::LoggedIn => (
            EventKind::Success,
            "已通过真实登录状态校验",
            "工作台将复用当前 Cookie 来源；登录态失效时需要重新校验或扫码登录。",
        ),
        SessionKind::Anonymous => (
            EventKind::Warning,
            "匿名风险已确认",
            "匿名模式可能导致评论、弹幕或部分接口结果不完整；工作台会持续显示该风险。",
        ),
        SessionKind::Unknown => (
            EventKind::Warning,
            "未登录风险",
            "未登录时不要默认认为采集完整；请扫码登录，或明确选择匿名进入并接受完整性风险。",
        ),
    };
    let color = event_color(kind, palette);

    h_flex()
        .items_start()
        .gap(px(10.))
        .p(px(12.))
        .rounded(px(16.))
        .border_1()
        .border_color(color.opacity(0.24))
        .bg(color.opacity(0.075))
        .child(status_dot(color))
        .child(
            v_flex()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(relative(1.35))
                        .text_color(palette.text)
                        .child(body),
                ),
        )
}

pub(crate) fn auth_lifecycle_block(auth: &AuthState, palette: &Palette) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap(px(10.))
        .child(auth_lifecycle_chip(
            "1",
            "登录状态校验",
            matches!(
                auth.phase,
                AuthPhase::BootChecking
                    | AuthPhase::LoggedIn
                    | AuthPhase::CredentialMissing
                    | AuthPhase::CredentialInvalid
                    | AuthPhase::CredentialError
            ),
            auth.status_kind(),
            palette,
        ))
        .child(auth_lifecycle_chip(
            "2",
            "扫码 / 凭据",
            auth.should_show_qr(),
            auth.status_kind(),
            palette,
        ))
        .child(auth_lifecycle_chip(
            "3",
            "进入工作台",
            auth.session.collection_ready().is_some(),
            if auth.session.collection_ready().is_some() {
                EventKind::Success
            } else {
                EventKind::System
            },
            palette,
        ))
}

fn auth_lifecycle_chip(
    step: &'static str,
    label: &'static str,
    active: bool,
    kind: EventKind,
    palette: &Palette,
) -> impl IntoElement {
    let color = event_color(kind, palette);
    let border_alpha = if active { 0.42 } else { 0.12 };
    let fill_alpha = if active { 0.14 } else { 0.035 };
    let step_fill_alpha = if active { 0.22 } else { 0.06 };
    let step_border_alpha = if active { 0.44 } else { 0.18 };

    h_flex()
        .flex_1()
        .items_center()
        .gap(px(8.))
        .min_w(px(0.))
        .p(px(10.))
        .rounded(px(18.))
        .border_1()
        .border_color(color.opacity(border_alpha))
        .bg(linear_gradient(
            135.,
            linear_color_stop(color.opacity(fill_alpha), 0.0),
            linear_color_stop(hsla(0., 0., 1., if active { 0.7 } else { 0.48 }), 1.0),
        ))
        .child(
            div()
                .flex_shrink_0()
                .size(px(24.))
                .rounded(px(999.))
                .border_1()
                .border_color(color.opacity(step_border_alpha))
                .bg(color.opacity(step_fill_alpha))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if active { color } else { palette.muted })
                        .child(step),
                ),
        )
        .child(
            div()
                .truncate()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if active { palette.text } else { palette.muted })
                .child(label),
        )
}

pub(crate) fn session_capsule(
    auth: &AuthState,
    palette: &Palette,
    image: Option<JellyCapsuleImage>,
) -> impl IntoElement {
    let kind = auth.status_kind();
    let color = event_color(kind, palette);
    let mut capsule = h_flex()
        .relative()
        .items_center()
        .gap(px(8.))
        .max_w(px(260.))
        .px(px(12.))
        .py(px(7.))
        .rounded(px(999.))
        .overflow_hidden()
        .border_1()
        .border_color(color.opacity(0.22))
        .bg(linear_gradient(
            135.,
            linear_color_stop(color.opacity(0.09), 0.0),
            linear_color_stop(hsla(0., 0., 1., 0.7), 1.0),
        ));

    if let Some(image) = image {
        capsule = capsule.child(
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
        );
    }

    capsule.child(status_dot(color)).child(
        v_flex()
            .relative()
            .min_w(px(0.))
            .gap(px(1.))
            .child(
                div()
                    .truncate()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color)
                    .child(auth.phase.label()),
            )
            .child(
                div()
                    .truncate()
                    .text_size(px(10.))
                    .text_color(palette.muted)
                    .child(auth.session.title()),
            ),
    )
}

pub(crate) fn qr_stage(auth: &AuthState, palette: &Palette, motion_tick: u64) -> impl IntoElement {
    if let Some(qr) = auth.qr.as_ref() {
        return qr_matrix_card(qr, auth.status_kind(), palette, motion_tick).into_any_element();
    }

    let pulse = wave_between(motion_tick, 0.16, 0.08, 0.2);
    div()
        .relative()
        .size(px(286.))
        .rounded(px(32.))
        .border_1()
        .border_color(palette.accent.opacity(0.16 + pulse))
        .bg(linear_gradient(
            145.,
            linear_color_stop(rgb(0xf6feff), 0.0),
            linear_color_stop(rgb(0xfff2f7), 1.0),
        ))
        .shadow_md()
        .flex()
        .items_center()
        .justify_center()
        .child(
            v_flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .size(px(64.))
                        .rounded(px(22.))
                        .border_1()
                        .border_color(palette.accent.opacity(0.26 + pulse))
                        .bg(palette.accent.opacity(0.08 + pulse * 0.35)),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.muted)
                        .child(match auth.phase {
                            AuthPhase::BootChecking => "正在校验",
                            AuthPhase::LoggedIn => "登录已确认",
                            AuthPhase::AnonymousAvailable => "匿名已确认",
                            _ => "等待二维码",
                        }),
                ),
        )
        .into_any_element()
}

fn qr_matrix_card(qr: &QrState, kind: EventKind, palette: &Palette, motion_tick: u64) -> gpui::Div {
    let card = 286.;
    let well = 246.;
    let canvas = 220.;
    let quiet_modules = 4.;
    let module = canvas / (qr.matrix.width as f32 + quiet_modules * 2.);
    let qr_offset = quiet_modules * module;
    let color = event_color(kind, palette);
    let pulse = wave_between(motion_tick, 0.18, 0.08, 0.22);
    let scan = wave_01(motion_tick, 0.2);
    let mut cells = Vec::new();
    for y in 0..qr.matrix.width {
        for x in 0..qr.matrix.width {
            if qr.matrix.is_dark(x, y) {
                cells.push(
                    div()
                        .absolute()
                        .left(px(qr_offset + x as f32 * module))
                        .top(px(qr_offset + y as f32 * module))
                        .size(px((module + 0.05).max(1.0)))
                        .bg(rgb(0x101828)),
                );
            }
        }
    }

    div()
        .relative()
        .size(px(card))
        .rounded(px(32.))
        .border_1()
        .border_color(color.opacity(0.3 + pulse))
        .bg(hsla(0., 0., 1., 0.78))
        .shadow(vec![gpui::BoxShadow {
            color: color.opacity(0.18 + pulse * 0.5),
            offset: gpui::point(px(0.), px(14.)),
            blur_radius: px(30.),
            spread_radius: px(-16.),
        }])
        .child(
            div()
                .absolute()
                .top(px((card - well) / 2.))
                .left(px((card - well) / 2.))
                .size(px(well))
                .rounded(px(22.))
                .bg(rgb(0xffffff))
                .border_1()
                .border_color(hsla(0., 0., 0.98, 0.92))
                .child(
                    div()
                        .absolute()
                        .top(px((well - canvas) / 2.))
                        .left(px((well - canvas) / 2.))
                        .size(px(canvas))
                        .children(cells),
                ),
        )
        .child(
            div()
                .absolute()
                .left(px(28. + scan * 46.))
                .right(px(72. - scan * 24.))
                .top(px(10.))
                .h(px(9.))
                .rounded(px(999.))
                .bg(hsla(0., 0., 1., 0.26 + pulse * 0.7)),
        )
}

pub(crate) fn qr_lifecycle_card(
    auth: &AuthState,
    palette: &Palette,
    motion_tick: u64,
) -> impl IntoElement {
    let color = event_color(auth.status_kind(), palette);
    let pulse = wave_between(motion_tick, 0.16, 0.05, 0.18);
    let title = match auth.phase {
        AuthPhase::QrWaitingForScan => "等待扫码",
        AuthPhase::QrWaitingForConfirm => "手机端待确认",
        AuthPhase::QrExpired => "二维码已过期",
        AuthPhase::QrSuccessChecking => "正在保存并复核",
        AuthPhase::LoggedIn => "登录态已确认",
        AuthPhase::AnonymousAvailable => "匿名模式已确认",
        AuthPhase::BootChecking => "正在校验登录态",
        AuthPhase::CredentialMissing => "未发现可用凭据",
        AuthPhase::CredentialInvalid => "当前凭据不可用",
        AuthPhase::CredentialError => "校验遇到异常",
    };
    let elapsed = auth.qr.as_ref().map(qr_elapsed_seconds);

    v_flex()
        .w_full()
        .gap(px(7.))
        .p(px(12.))
        .rounded(px(18.))
        .border_1()
        .border_color(color.opacity(0.22 + pulse))
        .bg(linear_gradient(
            135.,
            linear_color_stop(color.opacity(0.07 + pulse * 0.36), 0.0),
            linear_color_stop(hsla(0., 0., 1., 0.68), 1.0),
        ))
        .child(
            h_flex()
                .items_center()
                .gap(px(8.))
                .child(status_dot(color))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color)
                        .child(title),
                ),
        )
        .when_some(elapsed, |this, elapsed| {
            this.child(
                div()
                    .text_size(px(11.))
                    .text_color(palette.muted)
                    .child(format!(
                        "已等待 {elapsed} 秒；过期状态以 Bilibili 服务端轮询结果为准。"
                    )),
            )
        })
        .when_some(auth.qr.as_ref(), |this, qr| {
            this.child(
                div()
                    .text_size(px(11.))
                    .line_height(relative(1.3))
                    .text_color(palette.text)
                    .child(SharedString::from(qr.last_status_text.clone())),
            )
        })
}

pub(crate) fn qr_helper_copy(auth: &AuthState) -> String {
    match auth.phase {
        AuthPhase::QrWaitingForScan => {
            "请用 Bilibili 客户端扫码。二维码有效期有限，页面会持续检测，过期后这里会提供刷新入口。"
                .to_string()
        }
        AuthPhase::QrWaitingForConfirm => {
            "已扫码，请在手机端确认登录。如果手机端退出或二维码过期，页面会切到“刷新二维码”。"
                .to_string()
        }
        AuthPhase::QrExpired => {
            "二维码已过期。点击“刷新二维码”重新生成，不需要关闭应用或重启登录流程。".to_string()
        }
        AuthPhase::QrSuccessChecking => "授权已收到，正在保存登录状态并再次确认账号。".to_string(),
        AuthPhase::LoggedIn => "登录态已确认，可以进入工作台。".to_string(),
        AuthPhase::AnonymousAvailable => {
            "你已选择匿名进入；后续采集会持续显示完整性风险。".to_string()
        }
        _ => "未登录时可扫码登录，也可明确选择匿名进入。".to_string(),
    }
}

fn qr_elapsed_seconds(qr: &QrState) -> u64 {
    SystemTime::now()
        .duration_since(qr.generated_at)
        .unwrap_or_default()
        .as_secs()
}
