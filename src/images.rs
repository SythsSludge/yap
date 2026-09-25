//! Fetching, decoding and encoding image previews.
//!
//! Loading an image reveals your IP address to its host, so only hosts on the
//! trusted list are fetched, and every redirect hop is re-checked against it.

use crate::links::{Trust, check_trust};
use image::DynamicImage;
use ratatui::layout::Size;
use ratatui_image::picker::Picker;
use ratatui_image::sliced::SlicedProtocol;
use ratatui_image::{Resize, protocol::StatefulProtocol};
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

const MAX_REDIRECTS: usize = 5;
const MAX_DIMENSION: u32 = 8192;
const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct FetchPolicy {
    pub trusted: Vec<String>,
    pub https_only: bool,
    pub max_bytes: u64,
    /// A host the user explicitly approved for this one fetch.
    pub allow_host: Option<String>,
}

impl FetchPolicy {
    pub fn check(&self, url: &Url) -> Result<(), String> {
        let mut trusted = self.trusted.clone();
        trusted.extend(self.allow_host.clone());
        match check_trust(url, &trusted, self.https_only) {
            Trust::Trusted => Ok(()),
            Trust::UntrustedHost => Err(format!("{} is not a trusted image host", url.host_str().unwrap_or("?"))),
            Trust::InsecureScheme => Err("refusing to load an image over plain http".into()),
            Trust::NotHttp => Err("not an http(s) link".into()),
        }
    }
}

/// One HTTP round trip.
#[derive(Debug, PartialEq)]
pub enum Hop {
    Redirect(String),
    Body(Vec<u8>),
}

/// Follow redirects manually so every hop is checked against the policy.
pub fn follow(
    start: &Url,
    policy: &FetchPolicy,
    mut fetch: impl FnMut(&Url) -> Result<Hop, String>,
) -> Result<Vec<u8>, String> {
    let mut url = start.clone();
    for _ in 0..=MAX_REDIRECTS {
        policy.check(&url)?;
        match fetch(&url)? {
            Hop::Body(bytes) => return Ok(bytes),
            Hop::Redirect(location) => {
                url = url.join(&location).map_err(|e| format!("bad redirect: {e}"))?;
            }
        }
    }
    Err("too many redirects".into())
}

fn http_hop(agent: &ureq::Agent, url: &Url, max_bytes: u64) -> Result<Hop, String> {
    let mut resp = agent.get(url.as_str()).call().map_err(|e| e.to_string())?;
    let status = resp.status();
    if status.is_redirection() {
        let location =
            resp.headers().get("location").and_then(|v| v.to_str().ok()).ok_or("redirect without a location")?;
        return Ok(Hop::Redirect(location.to_owned()));
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    if let Some(ct) = resp.headers().get("content-type").and_then(|v| v.to_str().ok())
        && !ct.starts_with("image/")
        && !ct.starts_with("application/octet-stream")
    {
        return Err(format!("not an image ({ct})"));
    }
    resp.body_mut().with_config().limit(max_bytes).read_to_vec().map(Hop::Body).map_err(|e| match e {
        ureq::Error::BodyExceedsLimit(_) => format!("image is larger than {} KiB", max_bytes / 1024),
        e => e.to_string(),
    })
}

/// Blocking fetch; call from a blocking thread.
pub fn fetch(url: &Url, policy: &FetchPolicy) -> Result<Vec<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(TIMEOUT))
        .user_agent(format!("yap/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    follow(url, policy, |u| http_hop(&agent, u, policy.max_bytes))
}

/// Decode with limits so a hostile file can't exhaust memory.
pub fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|e| e.to_string())
}

/// A decoded image, pre-encoded for inline display.
pub struct Loaded {
    pub inline: SlicedProtocol,
    pub image: DynamicImage,
}

impl Loaded {
    pub fn rows(&self) -> u16 {
        self.inline.size().height
    }
}

pub fn prepare(picker: &Picker, image: DynamicImage, max: Size) -> Result<Loaded, String> {
    let inline =
        SlicedProtocol::new_with_resize(picker, image.clone(), max, Resize::Fit(None)).map_err(|e| e.to_string())?;
    Ok(Loaded { inline, image })
}

/// Everything from URL to displayable preview, on a blocking thread.
pub async fn load(url: Url, policy: FetchPolicy, picker: Picker, max: Size) -> Result<Loaded, String> {
    tokio::task::spawn_blocking(move || {
        let bytes = fetch(&url, &policy)?;
        prepare(&picker, decode(&bytes)?, max)
    })
    .await
    .map_err(|e| format!("image worker failed: {e}"))?
}

pub enum ImageState {
    Loading,
    Ready(Arc<Loaded>),
    Failed(String),
}

impl std::fmt::Debug for ImageState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageState::Loading => write!(f, "Loading"),
            ImageState::Ready(l) => write!(f, "Ready({}x{} px)", l.image.width(), l.image.height()),
            ImageState::Failed(e) => write!(f, "Failed({e})"),
        }
    }
}

/// The full-screen viewer; resizes to its area at render time.
pub struct Viewer {
    pub url: String,
    pub protocol: Option<StatefulProtocol>,
}

impl std::fmt::Debug for Viewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Viewer").field("url", &self.url).field("ready", &self.protocol.is_some()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, ImageFormat, Rgba};

    fn policy() -> FetchPolicy {
        FetchPolicy { trusted: vec!["good.com".into()], https_only: true, max_bytes: 1024, allow_host: None }
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_pixel(w, h, Rgba([255, 0, 128, 255]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn follows_redirects_within_trusted_hosts() {
        let start = Url::parse("https://good.com/a.png").unwrap();
        let mut seen = Vec::new();
        let body = follow(&start, &policy(), |u| {
            seen.push(u.to_string());
            Ok(match u.path() {
                "/a.png" => Hop::Redirect("https://cdn.good.com/b.png".into()),
                "/b.png" => Hop::Redirect("/c.png".into()),
                _ => Hop::Body(vec![1, 2, 3]),
            })
        })
        .unwrap();
        assert_eq!(body, vec![1, 2, 3]);
        assert_eq!(seen, vec!["https://good.com/a.png", "https://cdn.good.com/b.png", "https://cdn.good.com/c.png"]);
    }

    #[test]
    fn refuses_redirect_to_untrusted_or_insecure_host() {
        let start = Url::parse("https://good.com/a.png").unwrap();
        let err = follow(&start, &policy(), |_| Ok(Hop::Redirect("https://tracker.evil/x.png".into()))).unwrap_err();
        assert!(err.contains("tracker.evil"), "{err}");
        let err = follow(&start, &policy(), |_| Ok(Hop::Redirect("http://good.com/x.png".into()))).unwrap_err();
        assert!(err.contains("plain http"), "{err}");
    }

    #[test]
    fn never_fetches_untrusted_start() {
        let start = Url::parse("https://evil.com/a.png").unwrap();
        let err = follow(&start, &policy(), |_| panic!("must not fetch")).unwrap_err();
        assert!(err.contains("not a trusted"));
    }

    #[test]
    fn explicit_approval_allows_one_host() {
        let start = Url::parse("https://once.net/a.png").unwrap();
        let policy = FetchPolicy { allow_host: Some("once.net".into()), ..policy() };
        assert_eq!(follow(&start, &policy, |_| Ok(Hop::Body(vec![9]))).unwrap(), vec![9]);
    }

    #[test]
    fn stops_redirect_loops() {
        let start = Url::parse("https://good.com/a.png").unwrap();
        let err = follow(&start, &policy(), |_| Ok(Hop::Redirect("/a.png".into()))).unwrap_err();
        assert_eq!(err, "too many redirects");
    }

    #[test]
    fn decodes_png_and_rejects_garbage() {
        let img = decode(&png(4, 3)).unwrap();
        assert_eq!((img.width(), img.height()), (4, 3));
        assert!(decode(b"definitely not an image").is_err());
    }

    #[test]
    fn rejects_oversized_dimensions() {
        let err = decode(&png(MAX_DIMENSION + 1, 1)).unwrap_err();
        assert!(err.to_lowercase().contains("limit"), "{err}");
    }

    #[test]
    fn prepares_inline_preview_within_bounds() {
        let picker = Picker::halfblocks();
        let loaded = prepare(&picker, decode(&png(400, 100)).unwrap(), Size::new(20, 10)).unwrap();
        let size = loaded.inline.size();
        assert!(size.width <= 20 && size.height <= 10, "{size:?}");
        assert!(loaded.rows() >= 1);
    }
}
