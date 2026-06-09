use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use anyhow::Result;
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};

use super::client::{BiliClient, api_error_code};

const REPLY_MAIN_URL: &str = "https://api.bilibili.com/x/v2/reply/wbi/main";
const REPLY_LEGACY_MAIN_URL: &str = "https://api.bilibili.com/x/v2/reply/main";
const REPLY_COUNT_URL: &str = "https://api.bilibili.com/x/v2/reply/count";
const REPLY_DETAIL_URL: &str = "https://api.bilibili.com/x/v2/reply/reply";
const REPLY_PAGE_SIZE: u64 = 20;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommentBatch {
    pub comments: Vec<CommentRecord>,
    pub main_pages_scanned: usize,
    pub reply_pages_scanned: usize,
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
    #[serde(rename = "Pictures")]
    pub pictures: String,
    #[serde(rename = "Picture_count")]
    pub picture_count: usize,
    #[serde(rename = "Emotes")]
    pub emotes: String,
    #[serde(rename = "Emote_urls")]
    pub emote_urls: String,
    #[serde(rename = "At_users")]
    pub at_users: String,
    #[serde(rename = "Jump_url_keys")]
    pub jump_url_keys: String,
    #[serde(rename = "Jump_urls")]
    pub jump_urls: String,
    #[serde(rename = "Video_time_seconds")]
    pub video_time_seconds: String,
    #[serde(rename = "Video_time_links")]
    pub video_time_links: String,
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
    #[serde(skip)]
    pub reply_count: u64,
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
    #[serde(default)]
    rcount: u64,
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
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pictures: Vec<ReplyPictureData>,
    #[serde(default)]
    emote: BTreeMap<String, ReplyEmoteData>,
    #[serde(default)]
    at_name_to_mid: BTreeMap<String, u64>,
    #[serde(default)]
    jump_url: BTreeMap<String, ReplyJumpUrlData>,
}

#[derive(Debug, Deserialize)]
struct ReplyPictureData {
    #[serde(default)]
    img_src: String,
}

#[derive(Debug, Deserialize)]
struct ReplyEmoteData {
    #[serde(default)]
    url: String,
}

#[derive(Debug, Deserialize)]
struct ReplyJumpUrlData {
    #[serde(default)]
    pc_url: String,
    #[serde(default)]
    app_url_schema: String,
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

#[derive(Debug, Deserialize)]
struct ReplyCountData {
    count: u64,
}

impl BiliClient {
    pub async fn comment_count(&self, oid: u64) -> Result<u64> {
        let query = [("oid", oid.to_string()), ("type", "1".to_string())];
        let data: ReplyCountData = self.get_api(REPLY_COUNT_URL, &query).await?;
        Ok(data.count)
    }

    pub async fn main_comments(
        &self,
        bvid: &str,
        oid: u64,
        max_pages: Option<usize>,
        max_reply_pages: Option<usize>,
        request_delay: Option<Duration>,
    ) -> Result<CommentBatch> {
        let mut batch = match self.wbi_main_comment_page(bvid, oid, None).await {
            Ok(page) => {
                self.collect_wbi_main_comments(bvid, oid, page, max_pages, request_delay)
                    .await
            }
            Err(error) if api_error_code(&error) == Some(-101) => {
                tracing::warn!(
                    "WBI comment endpoint requires login; falling back to legacy endpoint"
                );
                let page = self.legacy_main_comment_page(bvid, oid, 0).await?;
                self.collect_legacy_main_comments(bvid, oid, page, max_pages, request_delay)
                    .await
            }
            Err(error) => Err(error),
        }?;

        let mut seen = batch
            .comments
            .iter()
            .map(|comment| comment.rpid)
            .collect::<HashSet<_>>();
        let roots = batch
            .comments
            .iter()
            .filter(|comment| comment.reply_count > 0)
            .map(|comment| (comment.rpid, comment.reply_count))
            .collect::<Vec<_>>();

        for (root, expected_count) in roots {
            let reply_batch = self
                .secondary_comments(
                    bvid,
                    oid,
                    root,
                    expected_count,
                    max_reply_pages,
                    request_delay,
                )
                .await?;
            batch.reply_pages_scanned += reply_batch.pages_scanned;
            push_unique_comments(&mut batch.comments, &mut seen, reply_batch.comments);
        }

        Ok(batch)
    }

    async fn collect_wbi_main_comments(
        &self,
        bvid: &str,
        oid: u64,
        first_page: CommentPage,
        max_pages: Option<usize>,
        request_delay: Option<Duration>,
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

            sleep_request_delay(request_delay).await;
            page = Some(self.wbi_main_comment_page(bvid, oid, Some(offset)).await?);
        }

        Ok(CommentBatch {
            comments,
            main_pages_scanned: pages_scanned,
            reply_pages_scanned: 0,
            next_cursor: next_cursor.map(|cursor| cursor.to_string()),
        })
    }

    async fn collect_legacy_main_comments(
        &self,
        bvid: &str,
        oid: u64,
        first_page: CommentPage,
        max_pages: Option<usize>,
        request_delay: Option<Duration>,
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

            sleep_request_delay(request_delay).await;
            page = Some(self.legacy_main_comment_page(bvid, oid, *next).await?);
        }

        Ok(CommentBatch {
            comments,
            main_pages_scanned: pages_scanned,
            reply_pages_scanned: 0,
            next_cursor: next_cursor.map(|cursor| cursor.to_string()),
        })
    }

    async fn secondary_comments(
        &self,
        bvid: &str,
        oid: u64,
        root: u64,
        expected_count: u64,
        max_reply_pages: Option<usize>,
        request_delay: Option<Duration>,
    ) -> Result<SecondaryCommentBatch> {
        let mut comments = Vec::new();
        let mut pages_scanned = 0;
        let mut pn = 1;

        loop {
            pages_scanned += 1;
            let page = self.secondary_comment_page(bvid, oid, root, pn).await?;
            let page_count = page.comments.len() as u64;
            comments.extend(page.comments);

            if reached_page_limit(pages_scanned, max_reply_pages) {
                break;
            }
            if page_count == 0 {
                break;
            }
            if comments.len() as u64 >= expected_count {
                break;
            }
            if page_count < REPLY_PAGE_SIZE {
                break;
            }

            pn += 1;
            sleep_request_delay(request_delay).await;
        }

        Ok(SecondaryCommentBatch {
            comments,
            pages_scanned,
        })
    }

    async fn secondary_comment_page(
        &self,
        bvid: &str,
        oid: u64,
        root: u64,
        pn: u64,
    ) -> Result<SecondaryCommentPage> {
        let query = [
            ("oid", oid.to_string()),
            ("pn", pn.to_string()),
            ("ps", REPLY_PAGE_SIZE.to_string()),
            ("root", root.to_string()),
            ("type", "1".to_string()),
        ];
        let data: ReplyMainData = self.get_api(REPLY_DETAIL_URL, &query).await?;
        Ok(SecondaryCommentPage::from_reply_data(bvid, data))
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

#[derive(Debug, Clone, Eq, PartialEq)]
struct SecondaryCommentBatch {
    comments: Vec<CommentRecord>,
    pages_scanned: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct SecondaryCommentPage {
    comments: Vec<CommentRecord>,
}

impl SecondaryCommentPage {
    fn from_reply_data(bvid: &str, data: ReplyMainData) -> Self {
        Self {
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

async fn sleep_request_delay(request_delay: Option<Duration>) {
    if let Some(delay) = request_delay
        && !delay.is_zero()
    {
        tokio::time::sleep(delay).await;
    }
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
        let rich_content = CommentRichContent::from_reply_content(bvid, &reply.content);

        Self {
            uname: reply.member.uname,
            sex: reply.member.sex,
            content: reply.content.message,
            pictures: rich_content.pictures,
            picture_count: rich_content.picture_count,
            emotes: rich_content.emotes,
            emote_urls: rich_content.emote_urls,
            at_users: rich_content.at_users,
            jump_url_keys: rich_content.jump_url_keys,
            jump_urls: rich_content.jump_urls,
            video_time_seconds: rich_content.video_time_seconds,
            video_time_links: rich_content.video_time_links,
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
            reply_count: reply.rcount,
        }
    }
}

struct CommentRichContent {
    pictures: String,
    picture_count: usize,
    emotes: String,
    emote_urls: String,
    at_users: String,
    jump_url_keys: String,
    jump_urls: String,
    video_time_seconds: String,
    video_time_links: String,
}

impl CommentRichContent {
    fn from_reply_content(bvid: &str, content: &ReplyContentData) -> Self {
        let picture_urls = content
            .pictures
            .iter()
            .filter_map(|picture| non_empty_string(&picture.img_src))
            .collect::<Vec<_>>();
        let emote_texts = content
            .emote
            .keys()
            .filter_map(|text| non_empty_string(text))
            .collect::<Vec<_>>();
        let emote_urls = content
            .emote
            .values()
            .filter_map(|emote| non_empty_string(&emote.url))
            .collect::<Vec<_>>();
        let at_users = content
            .at_name_to_mid
            .iter()
            .map(|(name, mid)| format!("{name}:{mid}"))
            .collect::<Vec<_>>();
        let jump_url_keys = content
            .jump_url
            .keys()
            .filter_map(|key| non_empty_string(key))
            .collect::<Vec<_>>();
        let jump_urls = content
            .jump_url
            .values()
            .filter_map(jump_url_value)
            .collect::<Vec<_>>();
        let video_times = detect_video_time_seconds(&content.message);
        let video_time_seconds = video_times
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let video_time_links = video_times
            .iter()
            .map(|seconds| format!("https://www.bilibili.com/video/{bvid}?t={seconds}"))
            .collect::<Vec<_>>();

        Self {
            picture_count: picture_urls.len(),
            pictures: join_field_values(picture_urls),
            emotes: join_field_values(emote_texts),
            emote_urls: join_field_values(emote_urls),
            at_users: join_field_values(at_users),
            jump_url_keys: join_field_values(jump_url_keys),
            jump_urls: join_field_values(jump_urls),
            video_time_seconds: join_field_values(video_time_seconds),
            video_time_links: join_field_values(video_time_links),
        }
    }
}

fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn jump_url_value(jump: &ReplyJumpUrlData) -> Option<String> {
    non_empty_string(&jump.pc_url)
        .map(normalize_protocol_relative_url)
        .or_else(|| non_empty_string(&jump.app_url_schema))
}

fn normalize_protocol_relative_url(url: String) -> String {
    if url.starts_with("//") {
        format!("https:{url}")
    } else {
        url
    }
}

fn join_field_values(values: Vec<String>) -> String {
    values.join(";")
}

fn detect_video_time_seconds(message: &str) -> Vec<u64> {
    let mut values = Vec::new();
    let mut candidate = String::new();

    for ch in message.chars() {
        if ch.is_ascii_digit() || ch == ':' {
            candidate.push(ch);
        } else {
            push_video_time_candidate(&mut values, &candidate);
            candidate.clear();
        }
    }
    push_video_time_candidate(&mut values, &candidate);

    values
}

fn push_video_time_candidate(values: &mut Vec<u64>, candidate: &str) {
    if let Some(seconds) = parse_video_time_candidate(candidate)
        && !values.contains(&seconds)
    {
        values.push(seconds);
    }
}

fn parse_video_time_candidate(candidate: &str) -> Option<u64> {
    if candidate.is_empty() || candidate.starts_with(':') || candidate.ends_with(':') {
        return None;
    }

    let parts = candidate.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [minutes, seconds] => {
            let minutes = minutes.parse::<u64>().ok()?;
            let seconds = parse_clock_part(seconds)?;
            Some(minutes * 60 + seconds)
        }
        [hours, minutes, seconds] => {
            let hours = hours.parse::<u64>().ok()?;
            let minutes = parse_clock_part(minutes)?;
            let seconds = parse_clock_part(seconds)?;
            Some(hours * 3600 + minutes * 60 + seconds)
        }
        _ => None,
    }
}

fn parse_clock_part(value: &str) -> Option<u64> {
    if value.len() != 2 {
        return None;
    }
    let value = value.parse::<u64>().ok()?;
    if value < 60 { Some(value) } else { None }
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
          "rcount": 2,
          "ctime": 1710000000,
          "like": 42,
          "content": {
            "message": "1:05:30 hello [吃瓜]",
            "pictures": [
              {
                "img_src": "http://i0.hdslb.com/bfs/new_dyn/sample.jpg"
              }
            ],
            "emote": {
              "[吃瓜]": {
                "url": "https://i0.hdslb.com/bfs/emote/sample.png"
              }
            },
            "at_name_to_mid": {
              "target": 4004
            },
            "jump_url": {
              "keyword": {
                "pc_url": "//search.bilibili.com/all?keyword=keyword",
                "app_url_schema": "bilibili://search?keyword=keyword"
              }
            }
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
        assert_eq!(record.content, "1:05:30 hello [吃瓜]");
        assert_eq!(
            record.pictures,
            "http://i0.hdslb.com/bfs/new_dyn/sample.jpg"
        );
        assert_eq!(record.picture_count, 1);
        assert_eq!(record.emotes, "[吃瓜]");
        assert_eq!(
            record.emote_urls,
            "https://i0.hdslb.com/bfs/emote/sample.png"
        );
        assert_eq!(record.at_users, "target:4004");
        assert_eq!(record.jump_url_keys, "keyword");
        assert_eq!(
            record.jump_urls,
            "https://search.bilibili.com/all?keyword=keyword"
        );
        assert_eq!(record.video_time_seconds, "3930");
        assert_eq!(
            record.video_time_links,
            "https://www.bilibili.com/video/BV1xx411c7mD?t=3930"
        );
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
        assert_eq!(record.reply_count, 2);
    }

    #[test]
    fn parses_reply_count_data() {
        let payload = r#"{ "count": 4689 }"#;
        let count: ReplyCountData = serde_json::from_str(payload).expect("count JSON");

        assert_eq!(count.count, 4689);
    }

    #[test]
    fn detects_video_time_candidates() {
        assert_eq!(detect_video_time_seconds("1:05:30 正题"), [3930]);
        assert_eq!(detect_video_time_seconds("12:47 看这里"), [767]);
        assert_eq!(detect_video_time_seconds("2:45:48 and 05:30"), [9948, 330]);
        assert!(detect_video_time_seconds("2026-06-09").is_empty());
        assert!(detect_video_time_seconds("99:99").is_empty());
    }
}
