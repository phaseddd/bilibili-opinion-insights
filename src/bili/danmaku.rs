use anyhow::{Context, Result};
use prost::Message;
use serde::Serialize;

use super::client::BiliClient;

const DANMAKU_SEGMENT_URL: &str = "https://api.bilibili.com/x/v2/dm/web/seg.so";
const SEGMENT_SECONDS: u64 = 360;

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
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
    pub pool: i32,
    pub id_str: String,
    pub attr: i32,
}

#[derive(Clone, PartialEq, Message)]
struct DmSegMobileReply {
    #[prost(message, repeated, tag = "1")]
    elems: Vec<DanmakuElemData>,
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
    ) -> Result<Vec<DanmakuRecord>> {
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
) -> Result<Vec<DanmakuRecord>> {
    let reply =
        DmSegMobileReply::decode(bytes).context("failed to decode Bilibili danmaku protobuf")?;
    Ok(reply
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
            pool: elem.pool,
            id_str: elem.id_str,
            attr: elem.attr,
        })
        .collect())
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
                action: String::new(),
                pool: 0,
                id_str: "1".to_string(),
                attr: 0,
            }],
        };
        let mut bytes = Vec::new();
        reply.encode(&mut bytes).expect("encode sample");

        let records = decode_danmaku_segment(
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

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].content, "hello");
        assert_eq!(records[0].progress_ms, 1000);
    }
}
