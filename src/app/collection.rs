use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::app::comments::{
    CommentCollectionOptions, CommentCollectionOutcome, CommentOutputFormat,
    collect_video_comments_with_events,
};
use crate::app::danmaku::{
    DanmakuCollectionOptions, DanmakuCollectionOutcome, collect_video_danmaku_with_events,
};
use crate::app::events::CollectionEvent;
use crate::bili::client::BiliClient;

pub const DEFAULT_COOKIE_PATH: &str = "config/bilibili-cookie.txt";
pub const DEFAULT_OUTPUT_ROOT: &str = "output";
pub const DEFAULT_REQUEST_DELAY: Duration = Duration::from_millis(500);
pub const DEFAULT_REQUEST_DELAY_MS: u64 = 500;

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct CredentialOptions {
    pub cookie: Option<PathBuf>,
    pub sessdata: Option<String>,
    pub anonymous: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CollectionRequest {
    pub bvids: Vec<String>,
    pub credentials: CredentialOptions,
    pub output: PathBuf,
    pub collect_comments: bool,
    pub collect_danmaku: bool,
    pub comment_formats: Vec<CommentOutputFormat>,
    pub max_comment_pages: Option<usize>,
    pub max_reply_pages: Option<usize>,
    pub max_danmaku_segments: Option<u64>,
    pub request_delay: Option<Duration>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CollectionRunOutcome {
    pub jobs: Vec<CollectionJobOutcome>,
    pub failures: Vec<CollectionFailure>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CollectionJobOutcome {
    Comments(CommentCollectionOutcome),
    Danmaku(DanmakuCollectionOutcome),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CollectionKind {
    Comments,
    Danmaku,
}

impl CollectionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Comments => "comments",
            Self::Danmaku => "danmaku",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CollectionFailure {
    pub bvid: String,
    pub kind: CollectionKind,
    pub error: String,
}

impl CollectionRunOutcome {
    pub fn ensure_success(&self) -> Result<()> {
        if self.failures.is_empty() {
            Ok(())
        } else {
            bail!("{} collection job(s) failed", self.failures.len())
        }
    }
}

pub async fn run_collection_with_events<F>(
    request: &CollectionRequest,
    mut on_event: F,
) -> Result<CollectionRunOutcome>
where
    F: FnMut(CollectionEvent) -> Result<()>,
{
    validate_collection_request(request)?;
    let cookie_header = load_cookie_header(&request.credentials)?;
    let client = BiliClient::new(cookie_header)?;
    let mut jobs = Vec::new();
    let mut failures = Vec::new();

    for bvid in &request.bvids {
        if request.collect_comments {
            let options = CommentCollectionOptions {
                output: request.output.clone(),
                formats: request.comment_formats.clone(),
                max_pages: request.max_comment_pages,
                max_reply_pages: request.max_reply_pages,
                request_delay: request.request_delay,
            };
            match collect_video_comments_with_events(&client, bvid, &options, &mut on_event).await {
                Ok(outcome) => jobs.push(CollectionJobOutcome::Comments(outcome)),
                Err(error) => failures.push(CollectionFailure {
                    bvid: bvid.clone(),
                    kind: CollectionKind::Comments,
                    error: error.to_string(),
                }),
            }
        }

        if request.collect_danmaku {
            let options = DanmakuCollectionOptions {
                output: request.output.clone(),
                max_segments: request.max_danmaku_segments,
                request_delay: request.request_delay,
            };
            match collect_video_danmaku_with_events(&client, bvid, &options, &mut on_event).await {
                Ok(outcome) => jobs.push(CollectionJobOutcome::Danmaku(outcome)),
                Err(error) => failures.push(CollectionFailure {
                    bvid: bvid.clone(),
                    kind: CollectionKind::Danmaku,
                    error: error.to_string(),
                }),
            }
        }
    }

    Ok(CollectionRunOutcome { jobs, failures })
}

pub fn validate_collection_request(request: &CollectionRequest) -> Result<()> {
    if request.bvids.is_empty() {
        bail!("provide at least one BVID or pass --input <FILE>");
    }
    if !request.collect_comments && !request.collect_danmaku {
        bail!("enable comments, danmaku, or both");
    }
    if request.collect_comments && request.comment_formats.is_empty() {
        bail!("enable at least one comment output format");
    }
    Ok(())
}

pub fn load_cookie_header(credentials: &CredentialOptions) -> Result<Option<String>> {
    match (
        credentials.cookie.as_ref(),
        credentials.sessdata.as_ref(),
        credentials.anonymous,
    ) {
        (Some(_), Some(_), _) => bail!("use either --cookie or --sessdata, not both"),
        (Some(_), _, true) | (_, Some(_), true) => {
            bail!("use --anonymous by itself, not with --cookie or --sessdata")
        }
        (_, _, true) => Ok(None),
        (Some(path), None, false) => read_cookie_file(path).map(Some),
        (None, Some(value), false) => {
            let sessdata = value.trim();
            if sessdata.is_empty() {
                bail!("--sessdata cannot be empty");
            }
            Ok(Some(format!("SESSDATA={sessdata}")))
        }
        (None, None, false) => default_cookie_header(),
    }
}

pub fn default_cookie_header() -> Result<Option<String>> {
    let path = Path::new(DEFAULT_COOKIE_PATH);
    if path.exists() {
        return read_cookie_file(path).map(Some);
    }
    Ok(None)
}

pub fn save_cookie_header(path: &Path, cookie_header: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create cookie directory: {}", parent.display()))?;
    }

    fs::write(path, cookie_header)
        .with_context(|| format!("failed to write cookie file: {}", path.display()))?;
    Ok(())
}

fn read_cookie_file(path: &Path) -> Result<String> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read cookie file: {}", path.display()))?
        .trim()
        .to_string();

    if content.is_empty() {
        bail!("cookie file is empty: {}", path.display());
    }

    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sessdata_as_cookie_header() {
        let cookie = load_cookie_header(&CredentialOptions {
            sessdata: Some("sample_sessdata".to_string()),
            ..CredentialOptions::default()
        })
        .expect("cookie header");

        assert_eq!(cookie, Some("SESSDATA=sample_sessdata".to_string()));
    }

    #[test]
    fn rejects_mixed_credentials() {
        let error = load_cookie_header(&CredentialOptions {
            cookie: Some(PathBuf::from("cookie.txt")),
            sessdata: Some("sample".to_string()),
            anonymous: false,
        })
        .expect_err("mixed credentials should fail");

        assert!(error.to_string().contains("either --cookie or --sessdata"));
    }

    #[test]
    fn rejects_empty_collection_plan() {
        let error = validate_collection_request(&CollectionRequest {
            bvids: vec!["BV1xx411c7mD".to_string()],
            credentials: CredentialOptions::default(),
            output: PathBuf::from(DEFAULT_OUTPUT_ROOT),
            collect_comments: false,
            collect_danmaku: false,
            comment_formats: vec![CommentOutputFormat::Csv],
            max_comment_pages: None,
            max_reply_pages: None,
            max_danmaku_segments: None,
            request_delay: Some(DEFAULT_REQUEST_DELAY),
        })
        .expect_err("empty collection plan should fail");

        assert!(error.to_string().contains("enable comments"));
    }
}
