use anyhow::Result;
use serde::Deserialize;

use super::client::BiliClient;

const VIDEO_VIEW_URL: &str = "https://api.bilibili.com/x/web-interface/view";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VideoInfo {
    pub bvid: String,
    pub aid: u64,
    pub cid: u64,
    pub title: String,
    pub comment_count: u64,
    pub danmaku_count: u64,
    pub pages: Vec<VideoPage>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VideoPage {
    pub cid: u64,
    pub page: u64,
    pub part: String,
    pub duration: u64,
}

#[derive(Debug, Deserialize)]
struct VideoViewData {
    bvid: String,
    aid: u64,
    cid: u64,
    title: String,
    #[serde(default)]
    stat: VideoStatData,
    #[serde(default)]
    pages: Vec<VideoPageData>,
}

#[derive(Debug, Default, Deserialize)]
struct VideoStatData {
    #[serde(default)]
    reply: u64,
    #[serde(default)]
    danmaku: u64,
}

#[derive(Debug, Deserialize)]
struct VideoPageData {
    cid: u64,
    page: u64,
    part: String,
    duration: u64,
}

impl BiliClient {
    pub async fn video_info(&self, bvid: &str) -> Result<VideoInfo> {
        let data: VideoViewData = self.get_api(VIDEO_VIEW_URL, &[("bvid", bvid)]).await?;
        Ok(data.into())
    }
}

impl From<VideoViewData> for VideoInfo {
    fn from(data: VideoViewData) -> Self {
        Self {
            bvid: data.bvid,
            aid: data.aid,
            cid: data.cid,
            title: data.title,
            comment_count: data.stat.reply,
            danmaku_count: data.stat.danmaku,
            pages: data.pages.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<VideoPageData> for VideoPage {
    fn from(data: VideoPageData) -> Self {
        Self {
            cid: data.cid,
            page: data.page,
            part: data.part,
            duration: data.duration,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_video_view_data() {
        let payload = r#"
        {
          "bvid": "BV1xx411c7mD",
          "aid": 123,
          "cid": 456,
          "title": "sample",
          "stat": {
            "reply": 7,
            "danmaku": 8
          },
          "pages": [
            {
              "cid": 456,
              "page": 1,
              "part": "P1",
              "duration": 90
            }
          ]
        }
        "#;

        let raw: VideoViewData = serde_json::from_str(payload).expect("sample video JSON");
        let info = VideoInfo::from(raw);

        assert_eq!(info.bvid, "BV1xx411c7mD");
        assert_eq!(info.aid, 123);
        assert_eq!(info.cid, 456);
        assert_eq!(info.title, "sample");
        assert_eq!(info.comment_count, 7);
        assert_eq!(info.danmaku_count, 8);
        assert_eq!(
            info.pages,
            [VideoPage {
                cid: 456,
                page: 1,
                part: "P1".to_string(),
                duration: 90,
            }]
        );
    }
}
