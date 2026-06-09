use anyhow::Result;
use serde::Deserialize;

use super::client::{BiliClient, api_error_code};

const NAV_URL: &str = "https://api.bilibili.com/x/web-interface/nav";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LoginState {
    pub is_login: bool,
    pub mid: Option<u64>,
    pub uname: Option<String>,
    pub vip_status: u64,
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
}
