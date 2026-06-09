use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::header::{ACCEPT, COOKIE, HeaderMap, HeaderValue, REFERER, USER_AGENT};
use serde::Deserialize;
use serde::de::DeserializeOwned;

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0 Safari/537.36";
const DEFAULT_REFERER: &str = "https://www.bilibili.com";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug)]
pub struct BiliClient {
    http: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct BiliApiResponse<T> {
    code: i64,
    message: String,
    data: Option<T>,
}

impl BiliClient {
    pub fn new(cookie_header: Option<String>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(REFERER, HeaderValue::from_static(DEFAULT_REFERER));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

        if let Some(cookie_header) = cookie_header {
            headers.insert(
                COOKIE,
                HeaderValue::from_str(cookie_header.trim())
                    .context("cookie header contains invalid characters")?,
            );
        }

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("failed to build Bilibili HTTP client")?;

        Ok(Self { http })
    }

    pub async fn get_api<T>(&self, url: &str, query: &[(&str, &str)]) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let response = self
            .http
            .get(url)
            .query(query)
            .send()
            .await
            .with_context(|| format!("failed to request Bilibili API: {url}"))?
            .error_for_status()
            .with_context(|| format!("Bilibili API returned a non-success HTTP status: {url}"))?;

        let payload: BiliApiResponse<T> = response
            .json()
            .await
            .with_context(|| format!("failed to parse Bilibili API JSON response: {url}"))?;

        if payload.code != 0 {
            bail!("Bilibili API error {}: {}", payload.code, payload.message);
        }

        payload
            .data
            .ok_or_else(|| anyhow!("Bilibili API response did not include data: {url}"))
    }
}
