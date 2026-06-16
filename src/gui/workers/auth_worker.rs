use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

use anyhow::Result;

use crate::app::collection::{
    CredentialOptions, DEFAULT_COOKIE_PATH, load_cookie_header, save_cookie_header,
};
use crate::bili::auth::{QrLoginSession, QrLoginStatus};
use crate::bili::client::BiliClient;
use crate::gui::messages::{AuthMessage, GuiMessage};
use crate::gui::state::auth::{AuthPhase, CredentialSource, QrMatrix, SessionKind, SessionMode};

const QR_POLL_INTERVAL: Duration = Duration::from_secs(2);

pub(crate) fn spawn_auth_bootstrap_worker(
    sender: mpsc::Sender<GuiMessage>,
    cancel: Arc<AtomicBool>,
    explicit_cookie: Option<PathBuf>,
) {
    std::thread::spawn(move || {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        let _ = sender.send(GuiMessage::Auth(AuthMessage::BootChecking));
        let message = run_auth_bootstrap_blocking(explicit_cookie).unwrap_or_else(|error| {
            AuthMessage::AuthError {
                phase: AuthPhase::CredentialError,
                message: format!("登录态校验失败：{error}"),
            }
        });
        if !cancel.load(Ordering::SeqCst) {
            let _ = sender.send(GuiMessage::Auth(message));
        }
    });
}

pub(crate) fn spawn_qr_generate_worker(sender: mpsc::Sender<GuiMessage>, cancel: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let message = run_qr_generate_blocking().unwrap_or_else(|error| AuthMessage::AuthError {
            phase: AuthPhase::CredentialError,
            message: format!("二维码生成失败：{error}"),
        });
        if !cancel.load(Ordering::SeqCst) {
            let _ = sender.send(GuiMessage::Auth(message));
        }
    });
}

pub(crate) fn spawn_qr_poll_worker(
    session: QrLoginSession,
    sender: mpsc::Sender<GuiMessage>,
    cancel: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let message = run_qr_poll_blocking(session, sender.clone(), cancel.clone())
            .err()
            .map(|error| AuthMessage::AuthError {
                phase: AuthPhase::CredentialError,
                message: format!("二维码登录失败：{error}"),
            });

        if let Some(message) = message
            && !cancel.load(Ordering::SeqCst)
        {
            let _ = sender.send(GuiMessage::Auth(message));
        }
    });
}

fn run_auth_bootstrap_blocking(explicit_cookie: Option<PathBuf>) -> Result<AuthMessage> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(auth_bootstrap(explicit_cookie))
}

fn run_qr_generate_blocking() -> Result<AuthMessage> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(generate_qr_login())
}

fn run_qr_poll_blocking(
    session: QrLoginSession,
    sender: mpsc::Sender<GuiMessage>,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(poll_qr_login(session, sender, cancel))
}

async fn auth_bootstrap(explicit_cookie: Option<PathBuf>) -> Result<AuthMessage> {
    let credentials = CredentialOptions {
        cookie: explicit_cookie.clone(),
        sessdata: None,
        anonymous: false,
    };
    let source = if explicit_cookie.is_some() {
        CredentialSource::ExplicitCookie
    } else {
        CredentialSource::DefaultCookie
    };
    let credential_exists = explicit_cookie
        .as_ref()
        .map(|path| path.exists())
        .unwrap_or_else(|| Path::new(DEFAULT_COOKIE_PATH).exists());
    let cookie_header = match load_cookie_header(&credentials) {
        Ok(cookie_header) => cookie_header,
        Err(error) if credential_exists => {
            return Ok(AuthMessage::AuthError {
                phase: AuthPhase::CredentialInvalid,
                message: format!("cookie 无法作为登录凭据使用：{error}"),
            });
        }
        Err(error) => return Err(error),
    };
    let source = if cookie_header.is_some() {
        source
    } else {
        CredentialSource::None
    };
    let client = BiliClient::new(cookie_header)?;
    let login = client.login_state().await?;
    let mut session = SessionMode::from_login_state(
        login,
        source,
        match source {
            CredentialSource::DefaultCookie => Some(PathBuf::from(DEFAULT_COOKIE_PATH)),
            CredentialSource::ExplicitCookie => explicit_cookie.clone(),
            _ => None,
        },
    );

    let (phase, message) = match (source, session.kind) {
        (CredentialSource::DefaultCookie, SessionKind::LoggedIn) => (
            AuthPhase::LoggedIn,
            format!("nav 已确认登录：{}", session.detail()),
        ),
        (CredentialSource::ExplicitCookie, SessionKind::LoggedIn) => (
            AuthPhase::LoggedIn,
            format!("nav 已确认显式 cookie 登录：{}", session.detail()),
        ),
        (CredentialSource::DefaultCookie, _) => {
            session.completeness_warning = true;
            (
                AuthPhase::CredentialInvalid,
                "默认 cookie 已读取，但 nav 未确认登录态；请扫码登录或匿名进入。".to_string(),
            )
        }
        (CredentialSource::ExplicitCookie, _) => {
            session.completeness_warning = true;
            (
                AuthPhase::CredentialInvalid,
                "显式 cookie 已读取，但 nav 未确认登录态；请更换 cookie、扫码登录或匿名进入。"
                    .to_string(),
            )
        }
        (CredentialSource::None, _) => (
            AuthPhase::CredentialMissing,
            "未发现默认 cookie；已调用 nav 确认为未登录，可扫码登录或匿名进入。".to_string(),
        ),
        _ => (AuthPhase::CredentialError, "登录态来源异常。".to_string()),
    };

    Ok(AuthMessage::NavChecked {
        phase,
        session,
        message,
    })
}

async fn generate_qr_login() -> Result<AuthMessage> {
    let client = BiliClient::new(None)?;
    let session = client.generate_qr_login().await?;
    let matrix = QrMatrix::from_session(&session)?;
    Ok(AuthMessage::QrGenerated {
        session,
        matrix,
        message: "二维码已生成，请使用 Bilibili 客户端扫码。".to_string(),
    })
}

async fn poll_qr_login(
    session: QrLoginSession,
    sender: mpsc::Sender<GuiMessage>,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    let client = BiliClient::new(None)?;

    loop {
        if cancel.load(Ordering::SeqCst) {
            return Ok(());
        }
        tokio::time::sleep(QR_POLL_INTERVAL).await;
        if cancel.load(Ordering::SeqCst) {
            return Ok(());
        }
        let status = client.poll_qr_login(&session.qrcode_key).await?;

        match status {
            QrLoginStatus::WaitingForScan => {
                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let _ = sender.send(GuiMessage::Auth(AuthMessage::QrStatus {
                    status: QrLoginStatus::WaitingForScan,
                    message: "等待扫码；二维码保持有效。".to_string(),
                }));
            }
            QrLoginStatus::WaitingForConfirm => {
                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let _ = sender.send(GuiMessage::Auth(AuthMessage::QrStatus {
                    status: QrLoginStatus::WaitingForConfirm,
                    message: "已扫码，等待手机端确认授权。".to_string(),
                }));
            }
            QrLoginStatus::Expired => {
                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let _ = sender.send(GuiMessage::Auth(AuthMessage::QrStatus {
                    status: QrLoginStatus::Expired,
                    message: "二维码已过期，请刷新二维码。".to_string(),
                }));
                return Ok(());
            }
            QrLoginStatus::Success { cookie_header } => {
                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let cookie_path = PathBuf::from(DEFAULT_COOKIE_PATH);
                let _ = sender.send(GuiMessage::Auth(AuthMessage::QrStatus {
                    status: QrLoginStatus::WaitingForConfirm,
                    message: "扫码授权成功，正在保存 cookie 并复核 nav。".to_string(),
                }));
                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                save_cookie_header(&cookie_path, &cookie_header)?;
                let _ = sender.send(GuiMessage::Auth(AuthMessage::QrCookieSaved {
                    path: cookie_path.clone(),
                    message: format!("cookie 已保存：{}", cookie_path.display()),
                }));

                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let client = BiliClient::new(Some(cookie_header))?;
                let login = client.login_state().await?;
                let session = SessionMode::from_login_state(
                    login,
                    CredentialSource::QrLogin,
                    Some(cookie_path),
                );
                let message = if session.kind == SessionKind::LoggedIn {
                    format!("保存后 nav 已确认登录：{}", session.detail())
                } else {
                    "保存后 nav 未确认登录态。".to_string()
                };
                let _ = sender.send(GuiMessage::Auth(AuthMessage::QrNavRechecked {
                    session,
                    message,
                }));
                return Ok(());
            }
        }
    }
}
