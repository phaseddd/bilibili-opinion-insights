use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;
use qrcode::{Color as QrColor, QrCode};

use crate::bili::auth::{LoginState, QrLoginSession, QrLoginStatus};
use crate::gui::state::events::EventKind;

#[derive(Default)]
pub(crate) struct AuthState {
    pub(crate) phase: AuthPhase,
    pub(crate) session: SessionMode,
    pub(crate) credential_source: CredentialSource,
    pub(crate) message: Option<String>,
    pub(crate) nav_error: Option<String>,
    pub(crate) qr: Option<QrState>,
    pub(crate) last_checked_at: Option<SystemTime>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SessionMode {
    pub(crate) kind: SessionKind,
    pub(crate) source: Option<CredentialSource>,
    pub(crate) cookie_path: Option<PathBuf>,
    pub(crate) mid: Option<u64>,
    pub(crate) uname: Option<String>,
    pub(crate) vip_status: u64,
    pub(crate) checked_at: Option<SystemTime>,
    pub(crate) completeness_warning: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum SessionKind {
    #[default]
    Unknown,
    LoggedIn,
    Anonymous,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum CredentialSource {
    #[default]
    None,
    DefaultCookie,
    ExplicitCookie,
    QrLogin,
    Anonymous,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum AuthPhase {
    #[default]
    BootChecking,
    LoggedIn,
    AnonymousAvailable,
    CredentialMissing,
    CredentialInvalid,
    CredentialError,
    QrWaitingForScan,
    QrWaitingForConfirm,
    QrExpired,
    QrSuccessChecking,
}

pub(crate) struct QrState {
    pub(crate) status: QrLoginStatus,
    pub(crate) matrix: QrMatrix,
    pub(crate) generated_at: SystemTime,
    pub(crate) last_polled_at: Option<SystemTime>,
    pub(crate) last_status_text: String,
}

pub(crate) struct QrMatrix {
    pub(crate) width: usize,
    modules: Vec<QrColor>,
}

impl QrMatrix {
    pub(crate) fn from_session(session: &QrLoginSession) -> Result<Self> {
        let code = QrCode::new(session.url.as_bytes())?;
        Ok(Self {
            width: code.width(),
            modules: code.to_colors(),
        })
    }

    pub(crate) fn is_dark(&self, x: usize, y: usize) -> bool {
        self.modules[y * self.width + x] == QrColor::Dark
    }
}

impl AuthState {
    pub(crate) fn set_phase(&mut self, phase: AuthPhase, message: impl Into<String>) {
        self.phase = phase;
        self.message = Some(message.into());
        self.nav_error = None;
        self.last_checked_at = Some(SystemTime::now());
    }

    pub(crate) fn set_error(&mut self, phase: AuthPhase, message: impl Into<String>) {
        self.phase = phase;
        self.nav_error = Some(message.into());
        self.message = self.nav_error.clone();
        self.last_checked_at = Some(SystemTime::now());
    }

    pub(crate) fn status_kind(&self) -> EventKind {
        match self.phase {
            AuthPhase::LoggedIn => EventKind::Success,
            AuthPhase::AnonymousAvailable
            | AuthPhase::CredentialMissing
            | AuthPhase::CredentialInvalid
            | AuthPhase::QrExpired => EventKind::Warning,
            AuthPhase::CredentialError => EventKind::Failure,
            AuthPhase::BootChecking
            | AuthPhase::QrWaitingForScan
            | AuthPhase::QrWaitingForConfirm
            | AuthPhase::QrSuccessChecking => EventKind::Danmaku,
        }
    }

    pub(crate) fn is_busy(&self) -> bool {
        matches!(
            self.phase,
            AuthPhase::BootChecking
                | AuthPhase::QrWaitingForScan
                | AuthPhase::QrWaitingForConfirm
                | AuthPhase::QrSuccessChecking
        )
    }

    pub(crate) fn should_show_qr(&self) -> bool {
        matches!(
            self.phase,
            AuthPhase::QrWaitingForScan
                | AuthPhase::QrWaitingForConfirm
                | AuthPhase::QrExpired
                | AuthPhase::QrSuccessChecking
        ) || self.qr.is_some()
    }
}

impl SessionMode {
    pub(crate) fn from_login_state(
        login: LoginState,
        source: CredentialSource,
        cookie_path: Option<PathBuf>,
    ) -> Self {
        let kind = if login.is_login {
            SessionKind::LoggedIn
        } else {
            SessionKind::Unknown
        };

        Self {
            kind,
            source: Some(source),
            cookie_path,
            mid: login.mid,
            uname: login.uname,
            vip_status: login.vip_status,
            checked_at: Some(SystemTime::now()),
            completeness_warning: false,
        }
    }

    pub(crate) fn anonymous() -> Self {
        Self {
            kind: SessionKind::Anonymous,
            source: Some(CredentialSource::Anonymous),
            cookie_path: None,
            mid: None,
            uname: None,
            vip_status: 0,
            checked_at: Some(SystemTime::now()),
            completeness_warning: true,
        }
    }

    pub(crate) fn collection_ready(&self) -> Option<Self> {
        matches!(self.kind, SessionKind::LoggedIn | SessionKind::Anonymous).then(|| self.clone())
    }

    pub(crate) fn title(&self) -> String {
        match self.kind {
            SessionKind::LoggedIn => self
                .uname
                .clone()
                .unwrap_or_else(|| "已登录账号".to_string()),
            SessionKind::Anonymous => "匿名模式".to_string(),
            SessionKind::Unknown => "未确认身份".to_string(),
        }
    }

    pub(crate) fn detail(&self) -> String {
        match self.kind {
            SessionKind::LoggedIn => {
                let mid = self
                    .mid
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "未知".to_string());
                let source = self
                    .source
                    .map(credential_source_label)
                    .unwrap_or("未知来源");
                format!("mid={mid} · VIP={} · {source}", self.vip_status)
            }
            SessionKind::Anonymous => {
                "匿名请求可能导致评论、弹幕或部分接口结果不完整。".to_string()
            }
            SessionKind::Unknown => "尚未完成登录状态校验。".to_string(),
        }
    }
}

impl AuthPhase {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::BootChecking => "校验中",
            Self::LoggedIn => "已登录",
            Self::AnonymousAvailable => "匿名模式",
            Self::CredentialMissing => "缺少凭据",
            Self::CredentialInvalid => "凭据失效",
            Self::CredentialError => "校验异常",
            Self::QrWaitingForScan => "等待扫码",
            Self::QrWaitingForConfirm => "等待确认",
            Self::QrExpired => "二维码过期",
            Self::QrSuccessChecking => "保存复核中",
        }
    }
}

pub(crate) fn credential_source_label(source: CredentialSource) -> &'static str {
    match source {
        CredentialSource::None => "无凭据",
        CredentialSource::DefaultCookie => "默认 Cookie 文件",
        CredentialSource::ExplicitCookie => "手动指定的 Cookie",
        CredentialSource::QrLogin => "二维码登录",
        CredentialSource::Anonymous => "匿名",
    }
}
