use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde::Deserialize;

use super::client::BiliClient;

const NAV_URL: &str = "https://api.bilibili.com/x/web-interface/nav";
const ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct WbiSigner {
    mixin_key: String,
}

#[derive(Debug, Deserialize)]
struct NavData {
    wbi_img: WbiImageData,
}

#[derive(Debug, Deserialize)]
struct WbiImageData {
    img_url: String,
    sub_url: String,
}

impl BiliClient {
    pub async fn wbi_signer(&self) -> Result<WbiSigner> {
        let query: &[(&str, &str)] = &[];
        let data: NavData = self.get_api(NAV_URL, query).await?;
        WbiSigner::from_image_urls(&data.wbi_img.img_url, &data.wbi_img.sub_url)
    }
}

impl WbiSigner {
    pub fn from_image_urls(img_url: &str, sub_url: &str) -> Result<Self> {
        let img_key = extract_key(img_url).context("failed to extract WBI img key")?;
        let sub_key = extract_key(sub_url).context("failed to extract WBI sub key")?;
        let raw_key = format!("{img_key}{sub_key}");
        let chars: Vec<char> = raw_key.chars().collect();

        if chars.len() < MIXIN_KEY_ENC_TAB.len() {
            bail!("WBI raw key is shorter than expected");
        }

        let mixin_key: String = MIXIN_KEY_ENC_TAB
            .iter()
            .take(32)
            .map(|index| chars[*index])
            .collect();

        Ok(Self { mixin_key })
    }

    pub fn sign(&self, mut params: BTreeMap<String, String>) -> Result<BTreeMap<String, String>> {
        params.insert("wts".to_string(), current_unix_seconds()?.to_string());
        Ok(self.sign_with_timestamp(params))
    }

    fn sign_with_timestamp(
        &self,
        mut params: BTreeMap<String, String>,
    ) -> BTreeMap<String, String> {
        let query = encode_params(&params);
        let w_rid = format!("{:x}", md5::compute(format!("{query}{}", self.mixin_key)));
        params.insert("w_rid".to_string(), w_rid);
        params
    }
}

fn extract_key(url: &str) -> Result<String> {
    let file_name = url
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .context("WBI URL has no file name")?;
    let key = file_name
        .split('.')
        .next()
        .filter(|value| !value.is_empty())
        .context("WBI URL file name has no key")?;
    Ok(key.to_string())
}

fn current_unix_seconds() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX_EPOCH")?
        .as_secs())
}

fn encode_params(params: &BTreeMap<String, String>) -> String {
    params
        .iter()
        .map(|(key, value)| {
            let sanitized = sanitize_value(value);
            format!(
                "{}={}",
                utf8_percent_encode(key, ENCODE_SET),
                utf8_percent_encode(&sanitized, ENCODE_SET)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn sanitize_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '!' | '\'' | '(' | ')' | '*'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_wbi_key_from_image_url() {
        let key = extract_key("https://i0.hdslb.com/bfs/wbi/abc123.png").expect("key");
        assert_eq!(key, "abc123");
    }

    #[test]
    fn builds_expected_mixin_key_length() {
        let signer = WbiSigner::from_image_urls(
            "https://i0.hdslb.com/bfs/wbi/abcdefghijklmnopqrstuvwxyzABCDEF.png",
            "https://i0.hdslb.com/bfs/wbi/GHIJKLMNOPQRSTUVWXYZabcdefghijkl.png",
        )
        .expect("signer");

        assert_eq!(signer.mixin_key.chars().count(), 32);
    }

    #[test]
    fn sanitizes_reserved_characters_before_encoding() {
        let mut params = BTreeMap::new();
        params.insert("message".to_string(), "a b!'()*中文".to_string());

        assert_eq!(encode_params(&params), "message=a%20b%E4%B8%AD%E6%96%87");
    }
}
