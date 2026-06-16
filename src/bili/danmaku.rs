use anyhow::{Context, Result};
use prost::Message;
use serde::{Deserialize, Serialize};

use super::client::BiliClient;

const DANMAKU_SEGMENT_URL: &str = "https://api.bilibili.com/x/v2/dm/web/seg.so";
const SEGMENT_SECONDS: u64 = 360;

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
pub struct DanmakuRecord {
    pub bvid: String,
    pub aid: u64,
    pub cid: u64,
    pub page: u64,
    pub part: String,
    pub segment_index: u64,
    pub id: i64,
    pub progress_ms: i32,
    pub mode: i32,
    pub font_size: i32,
    pub color: u32,
    pub mid_hash: String,
    pub content: String,
    pub ctime: i64,
    pub weight: i32,
    pub action: String,
    pub pool: i32,
    pub id_str: String,
    pub attr: i32,
    pub animation: String,
    pub colorful: i32,
}

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
pub struct DanmakuSegmentMetadata {
    pub bvid: String,
    pub aid: u64,
    pub cid: u64,
    pub page: u64,
    pub part: String,
    pub segment_index: u64,
    pub record_count: usize,
    pub state: i32,
    pub ai_flags: Vec<DanmakuAiFlagRecord>,
    pub colorful_sources: Vec<DanmakuColorfulSource>,
}

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
pub struct DanmakuAiFlagRecord {
    pub dmid: i64,
    pub flag: u32,
}

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
pub struct DanmakuColorfulSource {
    pub colorful_type: i32,
    pub src: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DanmakuSegment {
    pub records: Vec<DanmakuRecord>,
    pub metadata: DanmakuSegmentMetadata,
}

#[derive(Clone, PartialEq, Message)]
struct DmSegMobileReply {
    #[prost(message, repeated, tag = "1")]
    elems: Vec<DanmakuElemData>,
    #[prost(int32, tag = "2")]
    state: i32,
    #[prost(message, optional, tag = "3")]
    ai_flag: Option<DanmakuAiFlagData>,
    #[prost(message, repeated, tag = "5")]
    colorful_src: Vec<DmColorfulData>,
}

#[derive(Clone, PartialEq, Message)]
struct DanmakuElemData {
    #[prost(int64, tag = "1")]
    id: i64,
    #[prost(int32, tag = "2")]
    progress: i32,
    #[prost(int32, tag = "3")]
    mode: i32,
    #[prost(int32, tag = "4")]
    fontsize: i32,
    #[prost(uint32, tag = "5")]
    color: u32,
    #[prost(string, tag = "6")]
    mid_hash: String,
    #[prost(string, tag = "7")]
    content: String,
    #[prost(int64, tag = "8")]
    ctime: i64,
    #[prost(int32, tag = "9")]
    weight: i32,
    #[prost(string, tag = "10")]
    action: String,
    #[prost(int32, tag = "11")]
    pool: i32,
    #[prost(string, tag = "12")]
    id_str: String,
    #[prost(int32, tag = "13")]
    attr: i32,
    #[prost(string, tag = "22")]
    animation: String,
    #[prost(enumeration = "DmColorfulType", tag = "24")]
    colorful: i32,
}

#[derive(Clone, PartialEq, Message)]
struct DanmakuAiFlagData {
    #[prost(message, repeated, tag = "1")]
    dm_flags: Vec<DanmakuFlagData>,
}

#[derive(Clone, PartialEq, Message)]
struct DanmakuFlagData {
    #[prost(int64, tag = "1")]
    dmid: i64,
    #[prost(uint32, tag = "2")]
    flag: u32,
}

#[derive(Clone, PartialEq, Message)]
struct DmColorfulData {
    #[prost(enumeration = "DmColorfulType", tag = "1")]
    colorful_type: i32,
    #[prost(string, tag = "2")]
    src: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, prost::Enumeration)]
#[repr(i32)]
enum DmColorfulType {
    None = 0,
    VipGradualColor = 60001,
}

pub struct DanmakuSegmentContext<'a> {
    pub bvid: &'a str,
    pub aid: u64,
    pub cid: u64,
    pub page: u64,
    pub part: &'a str,
    pub segment_index: u64,
}

impl BiliClient {
    pub async fn danmaku_segment(
        &self,
        context: DanmakuSegmentContext<'_>,
    ) -> Result<DanmakuSegment> {
        let query = [
            ("type", "1".to_string()),
            ("oid", context.cid.to_string()),
            ("pid", context.aid.to_string()),
            ("segment_index", context.segment_index.to_string()),
        ];
        let bytes = self.get_bytes(DANMAKU_SEGMENT_URL, &query).await?;
        decode_danmaku_segment(&bytes, context)
    }
}

pub fn segment_count(duration_seconds: u64) -> u64 {
    duration_seconds.div_ceil(SEGMENT_SECONDS).max(1)
}

fn decode_danmaku_segment(
    bytes: &[u8],
    context: DanmakuSegmentContext<'_>,
) -> Result<DanmakuSegment> {
    let reply =
        DmSegMobileReply::decode(bytes).context("failed to decode Bilibili danmaku protobuf")?;
    let metadata = DanmakuSegmentMetadata::from_reply(&reply, &context);
    let records = reply
        .elems
        .into_iter()
        .map(|elem| DanmakuRecord {
            bvid: context.bvid.to_string(),
            aid: context.aid,
            cid: context.cid,
            page: context.page,
            part: context.part.to_string(),
            segment_index: context.segment_index,
            id: elem.id,
            progress_ms: elem.progress,
            mode: elem.mode,
            font_size: elem.fontsize,
            color: elem.color,
            mid_hash: elem.mid_hash,
            content: elem.content,
            ctime: elem.ctime,
            weight: elem.weight,
            action: elem.action,
            pool: elem.pool,
            id_str: elem.id_str,
            attr: elem.attr,
            animation: elem.animation,
            colorful: elem.colorful,
        })
        .collect();

    Ok(DanmakuSegment { records, metadata })
}

impl DanmakuSegmentMetadata {
    fn from_reply(reply: &DmSegMobileReply, context: &DanmakuSegmentContext<'_>) -> Self {
        Self {
            bvid: context.bvid.to_string(),
            aid: context.aid,
            cid: context.cid,
            page: context.page,
            part: context.part.to_string(),
            segment_index: context.segment_index,
            record_count: reply.elems.len(),
            state: reply.state,
            ai_flags: reply
                .ai_flag
                .as_ref()
                .map(|flag| {
                    flag.dm_flags
                        .iter()
                        .map(|item| DanmakuAiFlagRecord {
                            dmid: item.dmid,
                            flag: item.flag,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            colorful_sources: reply
                .colorful_src
                .iter()
                .map(|item| DanmakuColorfulSource {
                    colorful_type: item.colorful_type,
                    src: item.src.clone(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_segment_count() {
        assert_eq!(segment_count(1), 1);
        assert_eq!(segment_count(360), 1);
        assert_eq!(segment_count(361), 2);
        assert_eq!(segment_count(720), 2);
    }

    #[test]
    fn decodes_danmaku_segment() {
        let reply = DmSegMobileReply {
            state: 1,
            ai_flag: Some(DanmakuAiFlagData {
                dm_flags: vec![DanmakuFlagData { dmid: 1, flag: 8 }],
            }),
            colorful_src: vec![DmColorfulData {
                colorful_type: DmColorfulType::VipGradualColor as i32,
                src: "https://example.com/colorful.png".to_string(),
            }],
            elems: vec![DanmakuElemData {
                id: 1,
                progress: 1000,
                mode: 1,
                fontsize: 25,
                color: 16_777_215,
                mid_hash: "hash".to_string(),
                content: "hello".to_string(),
                ctime: 1710000000,
                weight: 0,
                action: "sample-action".to_string(),
                pool: 0,
                id_str: "1".to_string(),
                attr: 0,
                animation: "sample-animation".to_string(),
                colorful: DmColorfulType::VipGradualColor as i32,
            }],
        };
        let mut bytes = Vec::new();
        reply.encode(&mut bytes).expect("encode sample");

        let segment = decode_danmaku_segment(
            &bytes,
            DanmakuSegmentContext {
                bvid: "BV1xx411c7mD",
                aid: 10,
                cid: 20,
                page: 1,
                part: "P1",
                segment_index: 1,
            },
        )
        .expect("decode");

        let records = segment.records;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].content, "hello");
        assert_eq!(records[0].progress_ms, 1000);
        assert_eq!(records[0].action, "sample-action");
        assert_eq!(records[0].animation, "sample-animation");
        assert_eq!(records[0].colorful, DmColorfulType::VipGradualColor as i32);
        assert_eq!(segment.metadata.bvid, "BV1xx411c7mD");
        assert_eq!(segment.metadata.segment_index, 1);
        assert_eq!(segment.metadata.record_count, 1);
        assert_eq!(segment.metadata.state, 1);
        assert_eq!(
            segment.metadata.ai_flags,
            [DanmakuAiFlagRecord { dmid: 1, flag: 8 }]
        );
        assert_eq!(
            segment.metadata.colorful_sources,
            [DanmakuColorfulSource {
                colorful_type: DmColorfulType::VipGradualColor as i32,
                src: "https://example.com/colorful.png".to_string()
            }]
        );
    }
}
