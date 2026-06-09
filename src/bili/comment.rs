use std::collections::{BTreeMap, HashSet};

use anyhow::Result;
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};

use super::client::{BiliClient, api_error_code};

const REPLY_MAIN_URL: &str = "https://api.bilibili.com/x/v2/reply/wbi/main";
const REPLY_LEGACY_MAIN_URL: &str = "https://api.bilibili.com/x/v2/reply/main";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommentBatch {
    pub comments: Vec<CommentRecord>,
    pub pages_scanned: usize,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct CommentPage {
    comments: Vec<CommentRecord>,
    next_cursor: Option<CommentCursor>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum CommentCursor {
    Wbi(String),
    Legacy(u64),
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct CommentRecord {
    #[serde(rename = "Uname")]
    pub uname: String,
    #[serde(rename = "Sex")]
    pub sex: String,
    #[serde(rename = "Content")]
    pub content: String,
    #[serde(rename = "Rpid")]
    pub rpid: u64,
    #[serde(rename = "Oid")]
    pub oid: u64,
    #[serde(rename = "Bvid")]
    pub bvid: String,
    #[serde(rename = "Mid")]
    pub mid: u64,
    #[serde(rename = "Parent")]
    pub parent: u64,
    #[serde(rename = "Fansgrade")]
    pub fans_grade: bool,
    #[serde(rename = "Ctime")]
    pub ctime: u64,
    #[serde(rename = "Like")]
    pub like: u64,
    #[serde(rename = "Following")]
    pub following: bool,
    #[serde(rename = "Current_level")]
    pub current_level: u64,
    #[serde(rename = "Location")]
    pub location: String,
}

#[derive(Debug, Deserialize)]
struct ReplyMainData {
    #[serde(default)]
    cursor: ReplyCursorData,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    replies: Vec<ReplyData>,
}

#[derive(Debug, Default, Deserialize)]
struct ReplyCursorData {
    #[serde(default)]
    pagination_reply: Option<PaginationReplyData>,
    #[serde(default)]
    next: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PaginationReplyData {
    next_offset: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReplyData {
    rpid: u64,
    oid: u64,
    mid: u64,
    parent: u64,
    ctime: u64,
    like: u64,
    content: ReplyContentData,
    member: ReplyMemberData,
    #[serde(default)]
    reply_control: ReplyControlData,
}

#[derive(Debug, Deserialize)]
struct ReplyContentData {
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct ReplyMemberData {
    #[serde(default)]
    uname: String,
    #[serde(default)]
    sex: String,
    #[serde(default)]
    following: u64,
    #[serde(default)]
    level_info: ReplyLevelInfoData,
    #[serde(default)]
    fans_detail: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct ReplyLevelInfoData {
    #[serde(default)]
    current_level: u64,
}

#[derive(Debug, Default, Deserialize)]
struct ReplyControlData {
    #[serde(default)]
    location: String,
}

impl BiliClient {
    pub async fn main_comments(
        &self,
        bvid: &str,
        oid: u64,
        max_pages: Option<usize>,
    ) -> Result<CommentBatch> {
        match self.wbi_main_comment_page(bvid, oid, None).await {
            Ok(page) => {
                self.collect_wbi_main_comments(bvid, oid, page, max_pages)
                    .await
            }
            Err(error) if api_error_code(&error) == Some(-101) => {
                tracing::warn!(
                    "WBI comment endpoint requires login; falling back to legacy endpoint"
                );
                let page = self.legacy_main_comment_page(bvid, oid, 0).await?;
                self.collect_legacy_main_comments(bvid, oid, page, max_pages)
                    .await
            }
            Err(error) => Err(error),
        }
    }

    async fn collect_wbi_main_comments(
        &self,
        bvid: &str,
        oid: u64,
        first_page: CommentPage,
        max_pages: Option<usize>,
    ) -> Result<CommentBatch> {
        let mut comments = Vec::new();
        let mut seen = HashSet::new();
        let mut seen_cursors = HashSet::new();
        let mut pages_scanned = 0;
        let mut page = Some(first_page);
        let mut next_cursor = None;

        while let Some(current_page) = page {
            pages_scanned += 1;
            push_unique_comments(&mut comments, &mut seen, current_page.comments);
            next_cursor = current_page.next_cursor;

            if reached_page_limit(pages_scanned, max_pages) {
                break;
            }

            let Some(CommentCursor::Wbi(offset)) = &next_cursor else {
                break;
            };
            if offset.is_empty() {
                break;
            }
            if !seen_cursors.insert(offset.clone()) {
                break;
            }

            page = Some(self.wbi_main_comment_page(bvid, oid, Some(offset)).await?);
        }

        Ok(CommentBatch {
            comments,
            pages_scanned,
            next_cursor: next_cursor.map(|cursor| cursor.to_string()),
        })
    }

    async fn collect_legacy_main_comments(
        &self,
        bvid: &str,
        oid: u64,
        first_page: CommentPage,
        max_pages: Option<usize>,
    ) -> Result<CommentBatch> {
        let mut comments = Vec::new();
        let mut seen = HashSet::new();
        let mut seen_cursors = HashSet::new();
        let mut pages_scanned = 0;
        let mut page = Some(first_page);
        let mut next_cursor = None;

        while let Some(current_page) = page {
            pages_scanned += 1;
            push_unique_comments(&mut comments, &mut seen, current_page.comments);
            next_cursor = current_page.next_cursor;

            if reached_page_limit(pages_scanned, max_pages) {
                break;
            }

            let Some(CommentCursor::Legacy(next)) = &next_cursor else {
                break;
            };
            if *next == 0 {
                break;
            }
            if !seen_cursors.insert(*next) {
                break;
            }

            page = Some(self.legacy_main_comment_page(bvid, oid, *next).await?);
        }

        Ok(CommentBatch {
            comments,
            pages_scanned,
            next_cursor: next_cursor.map(|cursor| cursor.to_string()),
        })
    }

    async fn wbi_main_comment_page(
        &self,
        bvid: &str,
        oid: u64,
        offset: Option<&str>,
    ) -> Result<CommentPage> {
        let signer = self.wbi_signer().await?;
        let mut params = BTreeMap::new();
        params.insert("mode".to_string(), "3".to_string());
        params.insert("oid".to_string(), oid.to_string());
        params.insert(
            "pagination_str".to_string(),
            serde_json::json!({ "offset": offset.unwrap_or("") }).to_string(),
        );
        params.insert("plat".to_string(), "1".to_string());
        params.insert("type".to_string(), "1".to_string());
        params.insert("web_location".to_string(), "1315875".to_string());

        let signed_params = signer.sign(params)?;
        let data: ReplyMainData = self.get_api(REPLY_MAIN_URL, &signed_params).await?;
        Ok(CommentPage::from_wbi_reply_data(bvid, data))
    }

    async fn legacy_main_comment_page(
        &self,
        bvid: &str,
        oid: u64,
        next: u64,
    ) -> Result<CommentPage> {
        let query = [
            ("mode", "3".to_string()),
            ("next", next.to_string()),
            ("oid", oid.to_string()),
            ("type", "1".to_string()),
        ];
        let data: ReplyMainData = self.get_api(REPLY_LEGACY_MAIN_URL, &query).await?;
        Ok(CommentPage::from_legacy_reply_data(bvid, data))
    }
}

impl CommentPage {
    fn from_wbi_reply_data(bvid: &str, data: ReplyMainData) -> Self {
        Self {
            next_cursor: data
                .cursor
                .pagination_reply
                .and_then(|pagination| pagination.next_offset)
                .filter(|offset| !offset.is_empty())
                .map(CommentCursor::Wbi),
            comments: data
                .replies
                .into_iter()
                .map(|reply| CommentRecord::from_reply(bvid, reply))
                .collect(),
        }
    }

    fn from_legacy_reply_data(bvid: &str, data: ReplyMainData) -> Self {
        Self {
            next_cursor: data
                .cursor
                .next
                .filter(|next| *next > 0)
                .map(CommentCursor::Legacy),
            comments: data
                .replies
                .into_iter()
                .map(|reply| CommentRecord::from_reply(bvid, reply))
                .collect(),
        }
    }
}

impl std::fmt::Display for CommentCursor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wbi(offset) => write!(formatter, "wbi:{offset}"),
            Self::Legacy(next) => write!(formatter, "legacy:{next}"),
        }
    }
}

fn push_unique_comments(
    comments: &mut Vec<CommentRecord>,
    seen: &mut HashSet<u64>,
    page_comments: Vec<CommentRecord>,
) {
    for comment in page_comments {
        if seen.insert(comment.rpid) {
            comments.push(comment);
        }
    }
}

fn reached_page_limit(pages_scanned: usize, max_pages: Option<usize>) -> bool {
    max_pages.is_some_and(|limit| pages_scanned >= limit)
}

fn deserialize_null_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

impl CommentRecord {
    fn from_reply(bvid: &str, reply: ReplyData) -> Self {
        Self {
            uname: reply.member.uname,
            sex: reply.member.sex,
            content: reply.content.message,
            rpid: reply.rpid,
            oid: reply.oid,
            bvid: bvid.to_string(),
            mid: reply.mid,
            parent: reply.parent,
            fans_grade: reply.member.fans_detail.is_some(),
            ctime: reply.ctime,
            like: reply.like,
            following: reply.member.following != 0,
            current_level: reply.member.level_info.current_level,
            location: reply.reply_control.location,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_reply_to_comment_record() {
        let payload = r#"
        {
          "rpid": 1001,
          "oid": 2002,
          "mid": 3003,
          "parent": 0,
          "ctime": 1710000000,
          "like": 42,
          "content": {
            "message": "hello"
          },
          "member": {
            "uname": "tester",
            "sex": "保密",
            "following": 1,
            "level_info": {
              "current_level": 6
            },
            "fans_detail": {
              "is_fan": 1
            }
          },
          "reply_control": {
            "location": "IP属地：上海"
          }
        }
        "#;

        let reply: ReplyData = serde_json::from_str(payload).expect("sample reply JSON");
        let record = CommentRecord::from_reply("BV1xx411c7mD", reply);

        assert_eq!(record.uname, "tester");
        assert_eq!(record.sex, "保密");
        assert_eq!(record.content, "hello");
        assert_eq!(record.rpid, 1001);
        assert_eq!(record.oid, 2002);
        assert_eq!(record.bvid, "BV1xx411c7mD");
        assert_eq!(record.mid, 3003);
        assert_eq!(record.parent, 0);
        assert!(record.fans_grade);
        assert_eq!(record.ctime, 1710000000);
        assert_eq!(record.like, 42);
        assert!(record.following);
        assert_eq!(record.current_level, 6);
        assert_eq!(record.location, "IP属地：上海");
    }
}
