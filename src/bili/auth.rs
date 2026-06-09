use anyhow::{Context, Result, bail};
use qrcode::QrCode;
use qrcode::render::unicode;
use serde::Deserialize;

use super::client::{BiliClient, api_error_code, cookie_header_from_set_cookie};

const NAV_URL: &str = "https://api.bilibili.com/x/web-interface/nav";
const QR_GENERATE_URL: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/generate";
const QR_POLL_URL: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/poll";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LoginState {
    pub is_login: bool,
    pub mid: Option<u64>,
    pub uname: Option<String>,
    pub vip_status: u64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct QrLoginSession {
    pub url: String,
    pub qrcode_key: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum QrLoginStatus {
    WaitingForScan,
    WaitingForConfirm,
    Expired,
    Success { cookie_header: String },
}

#[derive(Debug, Deserialize)]
struct NavData {
    #[serde(default, rename = "isLogin")]
    is_login: bool,
    #[serde(default)]
    mid: Option<u64>,
    #[serde(default)]
    uname: Option<String>,
    #[serde(default, rename = "vipStatus")]
    vip_status: u64,
}

#[derive(Debug, Deserialize)]
struct QrGenerateData {
    url: String,
    qrcode_key: String,
}

#[derive(Debug, Deserialize)]
struct QrPollData {
    code: i64,
    #[serde(default)]
    message: String,
}

impl BiliClient {
    pub async fn login_state(&self) -> Result<LoginState> {
        let query: &[(&str, &str)] = &[];
        let result: Result<NavData> = self.get_api(NAV_URL, query).await;
        match result {
            Ok(data) => Ok(data.into()),
            Err(error) if api_error_code(&error) == Some(-101) => Ok(LoginState::anonymous()),
            Err(error) => Err(error),
        }
    }

    pub async fn generate_qr_login(&self) -> Result<QrLoginSession> {
        let query: &[(&str, &str)] = &[];
        let data: QrGenerateData = self.get_api(QR_GENERATE_URL, query).await?;
        Ok(QrLoginSession {
            url: data.url,
            qrcode_key: data.qrcode_key,
        })
    }

    pub async fn poll_qr_login(&self, qrcode_key: &str) -> Result<QrLoginStatus> {
        let query = [("qrcode_key", qrcode_key)];
        let response = self
            .get_api_with_headers::<QrPollData, _>(QR_POLL_URL, &query)
            .await?;

        match response.data.code {
            0 => {
                let cookie_header = cookie_header_from_set_cookie(&response.headers)
                    .context("login succeeded but no Set-Cookie header was returned")?;
                Ok(QrLoginStatus::Success { cookie_header })
            }
            86038 => Ok(QrLoginStatus::Expired),
            86090 => Ok(QrLoginStatus::WaitingForConfirm),
            86101 => Ok(QrLoginStatus::WaitingForScan),
            code => bail!(
                "unexpected QR login status {code}: {}",
                response.data.message
            ),
        }
    }
}

impl LoginState {
    fn anonymous() -> Self {
        Self {
            is_login: false,
            mid: None,
            uname: None,
            vip_status: 0,
        }
    }
}

impl From<NavData> for LoginState {
    fn from(data: NavData) -> Self {
        Self {
            is_login: data.is_login,
            mid: data.mid,
            uname: data.uname,
            vip_status: data.vip_status,
        }
    }
}

pub fn render_terminal_qr(url: &str) -> Result<String> {
    let code = QrCode::new(url.as_bytes()).context("failed to build login QR code")?;
    Ok(code.render::<unicode::Dense1x2>().quiet_zone(false).build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_login_state() {
        let payload = r#"
        {
          "isLogin": true,
          "mid": 123,
          "uname": "tester",
          "vipStatus": 1
        }
        "#;

        let raw: NavData = serde_json::from_str(payload).expect("sample nav JSON");
        let state = LoginState::from(raw);

        assert!(state.is_login);
        assert_eq!(state.mid, Some(123));
        assert_eq!(state.uname.as_deref(), Some("tester"));
        assert_eq!(state.vip_status, 1);
    }

    #[test]
    fn renders_login_qr() {
        let qr = render_terminal_qr("https://example.com/login").expect("qr");
        assert!(!qr.trim().is_empty());
    }
}
