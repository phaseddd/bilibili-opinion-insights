use std::collections::BTreeMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::client::{BiliClient, api_error_code};

const REPLY_MAIN_URL: &str = "https://api.bilibili.com/x/v2/reply/wbi/main";
const REPLY_LEGACY_MAIN_URL: &str = "https://api.bilibili.com/x/v2/reply/main";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommentPage {
    pub comments: Vec<CommentRecord>,
    pub next_offset: Option<String>,
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
    #[serde(default)]
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
    pub async fn main_comment_page(&self, bvid: &str, oid: u64) -> Result<CommentPage> {
        match self.wbi_main_comment_page(bvid, oid).await {
            Ok(page) => Ok(page),
            Err(error) if api_error_code(&error) == Some(-101) => {
                tracing::warn!(
                    "WBI comment endpoint requires login; falling back to legacy endpoint"
                );
                self.legacy_main_comment_page(bvid, oid).await
            }
            Err(error) => Err(error),
        }
    }

    async fn wbi_main_comment_page(&self, bvid: &str, oid: u64) -> Result<CommentPage> {
        let signer = self.wbi_signer().await?;
        let mut params = BTreeMap::new();
        params.insert("mode".to_string(), "3".to_string());
        params.insert("oid".to_string(), oid.to_string());
        params.insert("pagination_str".to_string(), r#"{"offset":""}"#.to_string());
        params.insert("plat".to_string(), "1".to_string());
        params.insert("type".to_string(), "1".to_string());
        params.insert("web_location".to_string(), "1315875".to_string());

        let signed_params = signer.sign(params)?;
        let data: ReplyMainData = self.get_api(REPLY_MAIN_URL, &signed_params).await?;
        Ok(CommentPage::from_reply_data(bvid, data))
    }

    async fn legacy_main_comment_page(&self, bvid: &str, oid: u64) -> Result<CommentPage> {
        let query = [
            ("mode", "3".to_string()),
            ("next", "0".to_string()),
            ("oid", oid.to_string()),
            ("type", "1".to_string()),
        ];
        let data: ReplyMainData = self.get_api(REPLY_LEGACY_MAIN_URL, &query).await?;
        Ok(CommentPage::from_reply_data(bvid, data))
    }
}

impl CommentPage {
    fn from_reply_data(bvid: &str, data: ReplyMainData) -> Self {
        Self {
            next_offset: data
                .cursor
                .pagination_reply
                .and_then(|pagination| pagination.next_offset)
                .or_else(|| data.cursor.next.map(|next| next.to_string())),
            comments: data
                .replies
                .into_iter()
                .map(|reply| CommentRecord::from_reply(bvid, reply))
                .collect(),
        }
    }
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
