use pulldown_cmark::{html, CowStr, Event as MdEvent, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub const MAX_MARKDOWN_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_EMBEDDED_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_EMBEDDED_IMAGES: usize = 128;
pub const MAX_IMAGE_DIMENSION: u32 = 8192;
pub const MAX_IMAGE_PIXELS: u64 = 33_554_432;
pub const MAX_RENDER_PAGES: u32 = 4096;

pub const HLJS_JS: &str = include_str!("../assets/hljs/highlight.min.js");
pub const HLJS_LIGHT: &str = include_str!("../assets/hljs/github.min.css");
pub const HLJS_DARK: &str = include_str!("../assets/hljs/github-dark.min.css");
pub const HLJS_EXTRA_LANGS: &str = concat!(include_str!("../assets/hljs/delphi.min.js"));
pub const PREVIEW_ENHANCE_JS: &str = include_str!("../assets/enhance/preview-enhance.js");
const KATEX_JS: &str = include_str!("../assets/katex/katex.min.js");
const KATEX_CSS: &str = include_str!("../assets/katex/katex.inline.css");
const MERMAID_JS: &str = include_str!("../assets/mermaid/mermaid.min.js");

pub const PREVIEW_CSS: &str = r#"
:root { color-scheme: light dark; }
body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
  margin: 0; padding: 0;
  line-height: 1.6; font-size: 15px;
  color: #1a1a1a; background: #fff;
}
#app { max-width: 820px; margin: 0 auto; padding: 24px; }
#preview h1,#preview h2,#preview h3,#preview h4 { margin-top: 1.4em; }
#preview h1 { border-bottom: 1px solid #e1e4e8; padding-bottom: .3em; }
#preview h2 { border-bottom: 1px solid #e1e4e8; padding-bottom: .2em; }
#preview code { background: #f0f0f0; padding: 2px 6px; border-radius: 4px; font-size: 90%; }
#preview pre { background: #f6f8fa; padding: 16px; border-radius: 8px; overflow-x: auto; }
#preview pre code { background: none; padding: 0; font-size: 14px; }
#preview blockquote { border-left: 4px solid #ddd; margin: 0; padding: 0 1em; color: #666; }
#preview .markdown-alert-note,
#preview .markdown-alert-tip,
#preview .markdown-alert-important,
#preview .markdown-alert-warning,
#preview .markdown-alert-caution {
  margin: 1em 0;
  padding: 0.75em 1em;
  border-radius: 6px;
  color: inherit;
}
#preview .markdown-alert-title {
  display: flex;
  align-items: center;
  gap: .35em;
  margin: 0 0 .45em;
  font-weight: 600;
  line-height: 1.25;
}
#preview .markdown-alert-title + p { margin-top: 0; }
#preview .markdown-alert-note { border-color: #0969da; background: #ddf4ff; }
#preview .markdown-alert-tip { border-color: #1a7f37; background: #dafbe1; }
#preview .markdown-alert-important { border-color: #8250df; background: #fbefff; }
#preview .markdown-alert-warning { border-color: #9a6700; background: #fff8c5; }
#preview .markdown-alert-caution { border-color: #cf222e; background: #ffebe9; }
#preview .markdown-alert-note .markdown-alert-title { color: #0969da; }
#preview .markdown-alert-tip .markdown-alert-title { color: #1a7f37; }
#preview .markdown-alert-important .markdown-alert-title { color: #8250df; }
#preview .markdown-alert-warning .markdown-alert-title { color: #9a6700; }
#preview .markdown-alert-caution .markdown-alert-title { color: #cf222e; }
#preview .mdp-mark { border-radius: 3px; padding: 0 0.12em; background: #fff2a8; color: inherit; }
#preview mark.search-hit { border-radius: 3px; padding: 0 0.12em; background: #fff2a8; color: inherit; }
#preview mark.search-hit.current { background: #ffcc4d; color: #1a1a1a; }
#preview table { border-collapse: collapse; width: 100%; }
#preview .mdp-table-wrap {
  width: min(calc(100vw - 64px), 1280px);
  margin: 1em 0 1em 50%;
  transform: translateX(-50%);
  overflow-x: auto;
  -webkit-overflow-scrolling: touch;
}
#preview .mdp-table-wrap table { width: max-content; min-width: 100%; }
#preview table th, #preview table td { border: 1px solid #ddd; padding: 8px 12px; text-align: left; }
#preview table th { background: #f6f8fa; font-weight: 600; color: #1a1a1a; white-space: nowrap; }
#preview table td { min-width: 64px; max-width: 360px; vertical-align: top; overflow-wrap: break-word; }
#preview img { max-width: 100%; }
#preview .katex-display { overflow-x: auto; overflow-y: hidden; padding: 0.15em 0; }
#preview .mdp-mermaid { margin: 1.2em 0; overflow-x: auto; text-align: center; }
#preview .mdp-mermaid svg { max-width: 100%; height: auto; }
#preview .mdp-mermaid-error, #preview .mdp-math-error { color: #b42318; }
#preview hr { border: none; border-top: 1px solid #e1e4e8; margin: 2em 0; }
#preview a { color: #0969da; text-decoration: none; }
#preview a:hover { text-decoration: underline; }
#preview ul, #preview ol { padding-left: 2em; }
#preview input[type="checkbox"] { margin-right: 6px; }
"#;

pub const PREVIEW_DARK_CSS: &str = r#"
body { color: #d4d4d4; background: #1e1e1e; }
#preview a { color: #6cb6ff; }
#preview h1, #preview h2 { border-color: #333; }
#preview pre { background: #2d2d2d !important; }
#preview code:not(pre code) { background: #2d2d2d; }
#preview blockquote { border-color: #444; color: #aaa; }
#preview .markdown-alert-note,
#preview .markdown-alert-tip,
#preview .markdown-alert-important,
#preview .markdown-alert-warning,
#preview .markdown-alert-caution { background: #161b22; color: #d4d4d4; }
#preview .markdown-alert-note { border-color: #2f81f7; }
#preview .markdown-alert-tip { border-color: #3fb950; }
#preview .markdown-alert-important { border-color: #a371f7; }
#preview .markdown-alert-warning { border-color: #d29922; }
#preview .markdown-alert-caution { border-color: #f85149; }
#preview .markdown-alert-note .markdown-alert-title { color: #2f81f7; }
#preview .markdown-alert-tip .markdown-alert-title { color: #3fb950; }
#preview .markdown-alert-important .markdown-alert-title { color: #a371f7; }
#preview .markdown-alert-warning .markdown-alert-title { color: #d29922; }
#preview .markdown-alert-caution .markdown-alert-title { color: #f85149; }
#preview table th { background: #2d2d2d; color: #f0f0f0; }
#preview table td, #preview table th { border-color: #444; }
#preview hr { border-color: #333; }
"#;

const SNAPSHOT_READY_JS: &str = r#"
(function() {
  var config = window.__mdPreviewRenderConfig;
  var state = window.__mdPreviewRenderState = { status: 'pending' };

  function twoFrames() {
    return new Promise(function(resolve) {
      requestAnimationFrame(function() { requestAnimationFrame(resolve); });
    });
  }

  function decodeImages() {
    return Promise.all(Array.prototype.map.call(document.images, function(image) {
      if (typeof image.decode === 'function') return image.decode();
      if (image.complete && image.naturalWidth > 0) return Promise.resolve();
      return new Promise(function(resolve, reject) {
        image.addEventListener('load', resolve, { once: true });
        image.addEventListener('error', function() { reject(new Error('image decode failed')); }, { once: true });
      });
    }));
  }

  Promise.resolve()
    .then(function() {
      if (window.hljs && window.hljs.highlightAll) window.hljs.highlightAll();
      if (window.__enhancePreview) return window.__enhancePreview();
    })
    .then(function() { return document.fonts && document.fonts.ready; })
    .then(decodeImages)
    .then(twoFrames)
    .then(function() {
      var root = document.documentElement;
      var body = document.body;
      var totalHeight = Math.max(root.scrollHeight, body.scrollHeight);
      var totalPages = Math.max(1, Math.ceil(totalHeight / config.height));
      if (totalPages > config.maxPages) {
        throw new Error('document exceeds page limit');
      }
      if (config.page >= totalPages) {
        state.status = 'failed';
        state.reason = 'page-out-of-range';
        state.totalPages = totalPages;
        document.title = 'md-preview-page-out-of-range:' + totalPages;
        return;
      }
      body.style.minHeight = (totalPages * config.height) + 'px';
      window.scrollTo(0, config.page * config.height);
      return twoFrames().then(function() {
        var expectedOffset = config.page * config.height;
        if (Math.abs(window.scrollY - expectedOffset) > 0.5) {
          throw new Error('failed to reach exact page offset');
        }
        state.status = 'ready';
        state.totalPages = totalPages;
        state.totalHeight = totalHeight;
        state.viewportWidth = window.innerWidth;
        state.viewportHeight = window.innerHeight;
        document.title = 'md-preview-ready:' + totalPages + ':' + totalHeight + ':' +
          window.innerWidth + ':' + window.innerHeight;
      });
    })
    .catch(function(error) {
      state.status = 'failed';
      state.reason = String(error && error.message || error);
      document.title = 'md-preview-failed:' + state.reason.slice(0, 160);
    });
})();
"#;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct EnhanceFlags {
    pub math: bool,
    pub mermaid: bool,
}

impl EnhanceFlags {
    pub fn any(self) -> bool {
        self.math || self.mermaid
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PreviewTheme {
    Light,
    Dark,
}

impl PreviewTheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

#[derive(Debug)]
pub struct RenderedMarkdown {
    pub html: String,
    pub flags: EnhanceFlags,
}

#[derive(Copy, Clone, Debug)]
pub struct SnapshotPageOptions {
    pub page: u32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub theme: PreviewTheme,
    pub max_pages: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SnapshotInputError {
    RawHtml(String),
    UnsafeImage { url: String, reason: String },
    TooManyImages,
    ImageBytesExceeded,
}

impl fmt::Display for SnapshotInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RawHtml(fragment) => {
                write!(f, "raw HTML is not allowed in snapshot mode: {fragment}")
            }
            Self::UnsafeImage { url, reason } => write!(f, "unsafe image {url:?}: {reason}"),
            Self::TooManyImages => write!(f, "document exceeds the embedded image count limit"),
            Self::ImageBytesExceeded => {
                write!(f, "document exceeds the embedded image byte limit")
            }
        }
    }
}

impl std::error::Error for SnapshotInputError {}

pub fn md_to_html(md: &str) -> String {
    md_to_html_with_base(md, None)
}

pub fn md_to_html_with_base(md: &str, base_dir: Option<&Path>) -> String {
    let events = markdown_events(md);
    let events = embed_desktop_images(add_mark_highlights(add_heading_ids(events)), base_dir);
    events_to_html(events)
}

pub fn render_snapshot_markdown(
    md: &str,
    base_dir: &Path,
) -> Result<RenderedMarkdown, SnapshotInputError> {
    let events = markdown_events(md);
    reject_raw_html(&events)?;
    let events = add_mark_highlights(add_heading_ids(events));
    let events = embed_snapshot_images(events, base_dir)?;
    Ok(RenderedMarkdown {
        html: events_to_html(events),
        flags: enhance_flags_for(md),
    })
}

pub fn build_snapshot_page(rendered: &RenderedMarkdown, options: SnapshotPageOptions) -> String {
    let highlight_css = match options.theme {
        PreviewTheme::Light => HLJS_LIGHT,
        PreviewTheme::Dark => HLJS_DARK,
    };

    let mut page = String::with_capacity(
        rendered.html.len()
            + HLJS_JS.len()
            + HLJS_EXTRA_LANGS.len()
            + PREVIEW_ENHANCE_JS.len()
            + 32_768,
    );
    page.push_str("<!doctype html><html data-theme=\"");
    page.push_str(options.theme.as_str());
    page.push_str("\"><head><meta charset=\"utf-8\">");
    page.push_str("<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data:; font-src data:; style-src 'unsafe-inline'; script-src 'unsafe-inline' 'unsafe-eval'; connect-src 'none'; object-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'\">");
    page.push_str("<style>");
    page.push_str(PREVIEW_CSS);
    if options.theme == PreviewTheme::Dark {
        page.push_str(PREVIEW_DARK_CSS);
    }
    page.push_str("html{color-scheme:");
    page.push_str(options.theme.as_str());
    page.push_str(";overflow:hidden}body{min-height:100vh}#app{box-sizing:border-box}</style>");
    page.push_str("<style>");
    page.push_str(highlight_css);
    page.push_str("</style>");
    if rendered.flags.math {
        page.push_str("<style id=\"katex-css\">");
        page.push_str(KATEX_CSS);
        page.push_str("</style>");
    }
    page.push_str("</head><body><main id=\"app\"><article id=\"preview\">");
    page.push_str(&rendered.html);
    page.push_str("</article></main><script>");
    page.push_str(HLJS_JS);
    page.push('\n');
    page.push_str(HLJS_EXTRA_LANGS);
    page.push_str("\n;try{window.hljs=hljs;}catch(error){}\n");
    if rendered.flags.math {
        page.push_str(KATEX_JS);
        page.push_str("\n;try{window.katex=katex;}catch(error){}\n");
    }
    if rendered.flags.mermaid {
        page.push_str(MERMAID_JS);
        page.push_str("\n;try{window.mermaid=mermaid;}catch(error){}\n");
    }
    page.push_str("window.__mdPreviewFeatureFlags={math:");
    page.push_str(if rendered.flags.math { "true" } else { "false" });
    page.push_str(",mermaid:");
    page.push_str(if rendered.flags.mermaid {
        "true"
    } else {
        "false"
    });
    page.push_str("};\n");
    page.push_str(PREVIEW_ENHANCE_JS);
    page.push_str("\nwindow.__mdPreviewRenderConfig=");
    page.push_str(&format!(
        "{{page:{},width:{},height:{},scale:{},maxPages:{}}}",
        options.page, options.width, options.height, options.scale, options.max_pages
    ));
    page.push_str(";\n");
    page.push_str(SNAPSHOT_READY_JS);
    page.push_str("</script></body></html>");
    page
}

fn markdown_events(md: &str) -> Vec<MdEvent<'_>> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_MATH
        | Options::ENABLE_GFM;
    Parser::new_ext(md, options).collect()
}

fn events_to_html(events: Vec<MdEvent<'_>>) -> String {
    let mut output = String::new();
    html::push_html(&mut output, events.into_iter());
    output
}

fn reject_raw_html(events: &[MdEvent<'_>]) -> Result<(), SnapshotInputError> {
    for event in events {
        let fragment = match event {
            MdEvent::Html(fragment) | MdEvent::InlineHtml(fragment) => fragment,
            _ => continue,
        };
        let mut snippet = fragment
            .trim()
            .chars()
            .take(80)
            .map(|character| {
                if character.is_control() {
                    char::REPLACEMENT_CHARACTER
                } else {
                    character
                }
            })
            .collect::<String>();
        if fragment.trim().chars().count() > 80 {
            snippet.push('…');
        }
        return Err(SnapshotInputError::RawHtml(snippet));
    }
    Ok(())
}

fn embed_desktop_images<'a>(events: Vec<MdEvent<'a>>, base_dir: Option<&Path>) -> Vec<MdEvent<'a>> {
    let Some(base_dir) = base_dir else {
        return events;
    };

    events
        .into_iter()
        .map(|event| match event {
            MdEvent::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let embedded = desktop_local_image_data_url(base_dir, dest_url.as_ref());
                MdEvent::Start(Tag::Image {
                    link_type,
                    dest_url: embedded.map(CowStr::from).unwrap_or(dest_url),
                    title,
                    id,
                })
            }
            _ => event,
        })
        .collect()
}

fn embed_snapshot_images<'a>(
    events: Vec<MdEvent<'a>>,
    base_dir: &Path,
) -> Result<Vec<MdEvent<'a>>, SnapshotInputError> {
    let canonical_base =
        base_dir
            .canonicalize()
            .map_err(|error| SnapshotInputError::UnsafeImage {
                url: base_dir.display().to_string(),
                reason: format!("cannot canonicalize source directory: {error}"),
            })?;
    let mut image_count = 0usize;
    let mut total_bytes = 0u64;
    let mut output = Vec::with_capacity(events.len());

    for event in events {
        let MdEvent::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) = event
        else {
            output.push(event);
            continue;
        };

        image_count += 1;
        if image_count > MAX_EMBEDDED_IMAGES {
            return Err(SnapshotInputError::TooManyImages);
        }
        let url = dest_url.to_string();
        let image_path = resolve_snapshot_image_path(&canonical_base, &url)?;
        let metadata = image_path
            .metadata()
            .map_err(|error| SnapshotInputError::UnsafeImage {
                url: url.clone(),
                reason: format!("cannot read image metadata: {error}"),
            })?;
        if !metadata.is_file() {
            return Err(SnapshotInputError::UnsafeImage {
                url,
                reason: "image path is not a regular file".to_string(),
            });
        }
        if metadata.len() > MAX_IMAGE_BYTES {
            return Err(SnapshotInputError::UnsafeImage {
                url,
                reason: "image exceeds the per-file byte limit".to_string(),
            });
        }
        let mime = snapshot_image_mime_type(&image_path).ok_or_else(|| {
            SnapshotInputError::UnsafeImage {
                url: url.clone(),
                reason: "snapshot mode supports PNG, JPEG, and SVG images".to_string(),
            }
        })?;
        let file =
            fs::File::open(&image_path).map_err(|error| SnapshotInputError::UnsafeImage {
                url: url.clone(),
                reason: format!("cannot open image: {error}"),
            })?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(MAX_IMAGE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| SnapshotInputError::UnsafeImage {
                url: url.clone(),
                reason: format!("cannot read image: {error}"),
            })?;
        if bytes.len() as u64 > MAX_IMAGE_BYTES {
            return Err(SnapshotInputError::UnsafeImage {
                url,
                reason: "image exceeds the per-file byte limit".to_string(),
            });
        }
        validate_snapshot_image(mime, &bytes).map_err(|reason| {
            SnapshotInputError::UnsafeImage {
                url: url.clone(),
                reason,
            }
        })?;
        total_bytes = total_bytes
            .checked_add(bytes.len() as u64)
            .ok_or(SnapshotInputError::ImageBytesExceeded)?;
        if total_bytes > MAX_EMBEDDED_IMAGE_BYTES {
            return Err(SnapshotInputError::ImageBytesExceeded);
        }
        let embedded = format!("data:{mime};base64,{}", base64_encode(&bytes));
        output.push(MdEvent::Start(Tag::Image {
            link_type,
            dest_url: CowStr::Boxed(embedded.into_boxed_str()),
            title,
            id,
        }));
    }

    Ok(output)
}

fn desktop_local_image_data_url(base_dir: &Path, url: &str) -> Option<String> {
    let image_path = resolve_local_relative_image_path(base_dir, url)?;
    let mime = image_mime_type(&image_path)?;
    let bytes = fs::read(image_path).ok()?;
    Some(format!("data:{mime};base64,{}", base64_encode(&bytes)))
}

fn resolve_snapshot_image_path(
    canonical_base: &Path,
    url: &str,
) -> Result<PathBuf, SnapshotInputError> {
    let candidate = resolve_local_relative_image_path(canonical_base, url).ok_or_else(|| {
        SnapshotInputError::UnsafeImage {
            url: url.to_string(),
            reason: "only contained local relative images are allowed".to_string(),
        }
    })?;
    let canonical = candidate
        .canonicalize()
        .map_err(|error| SnapshotInputError::UnsafeImage {
            url: url.to_string(),
            reason: format!("cannot canonicalize image: {error}"),
        })?;
    if !canonical.starts_with(canonical_base) {
        return Err(SnapshotInputError::UnsafeImage {
            url: url.to_string(),
            reason: "image resolves outside the Markdown directory".to_string(),
        });
    }
    Ok(canonical)
}

fn resolve_local_relative_image_path(base_dir: &Path, url: &str) -> Option<PathBuf> {
    let path_part = url.split(['#', '?']).next()?.trim();
    if !is_local_relative_url(path_part) {
        return None;
    }

    let mut candidate = base_dir.to_path_buf();
    for segment in path_part.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        let decoded = percent_decode_path_segment(segment)?;
        let segment_path = Path::new(&decoded);
        if segment_path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return None;
        }
        candidate.push(segment_path);
    }

    if candidate == base_dir {
        return None;
    }
    Some(candidate)
}

fn is_local_relative_url(url: &str) -> bool {
    !url.is_empty()
        && !url.starts_with('#')
        && !url.starts_with('/')
        && !url.starts_with('\\')
        && !url.starts_with("//")
        && !url.contains(':')
}

fn percent_decode_path_segment(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes.get(index + 1).and_then(|byte| hex_value(*byte))?;
            let low = bytes.get(index + 2).and_then(|byte| hex_value(*byte))?;
            output.push((high << 4) | low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn image_mime_type(path: &Path) -> Option<&'static str> {
    match path
        .extension()?
        .to_string_lossy()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "bmp" => Some("image/bmp"),
        "ico" => Some("image/x-icon"),
        "avif" => Some("image/avif"),
        _ => None,
    }
}

fn snapshot_image_mime_type(path: &Path) -> Option<&'static str> {
    match path
        .extension()?
        .to_string_lossy()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

fn validate_snapshot_image(mime: &str, bytes: &[u8]) -> Result<(), String> {
    let (width, height) = match mime {
        "image/png" => {
            let (width, height) = png_dimensions(bytes)?;
            (f64::from(width), f64::from(height))
        }
        "image/jpeg" => {
            let (width, height) = jpeg_dimensions(bytes)?;
            (f64::from(width), f64::from(height))
        }
        "image/svg+xml" => svg_dimensions(bytes)?,
        _ => return Err("unsupported snapshot image type".to_string()),
    };
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err("image dimensions must be positive finite numbers".to_string());
    }
    if width > f64::from(MAX_IMAGE_DIMENSION) || height > f64::from(MAX_IMAGE_DIMENSION) {
        return Err(format!(
            "image dimensions exceed {MAX_IMAGE_DIMENSION} pixels"
        ));
    }
    if width * height > MAX_IMAGE_PIXELS as f64 {
        return Err(format!("image exceeds the {MAX_IMAGE_PIXELS} pixel limit"));
    }
    Ok(())
}

fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
    const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return Err("invalid PNG header".to_string());
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("fixed PNG width slice"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("fixed PNG height slice"));
    Ok((width, height))
}

fn jpeg_dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return Err("invalid JPEG header".to_string());
    }
    let mut index = 2usize;
    while index < bytes.len() {
        while bytes.get(index) == Some(&0xff) {
            index += 1;
        }
        let Some(&marker) = bytes.get(index) else {
            break;
        };
        index += 1;
        if marker == 0xd9 || marker == 0xda {
            break;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let length_bytes = bytes
            .get(index..index + 2)
            .ok_or_else(|| "truncated JPEG segment".to_string())?;
        let length = u16::from_be_bytes([length_bytes[0], length_bytes[1]]) as usize;
        if length < 2 || index + length > bytes.len() {
            return Err("invalid JPEG segment length".to_string());
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            if length < 7 {
                return Err("truncated JPEG frame header".to_string());
            }
            let height = u16::from_be_bytes([bytes[index + 3], bytes[index + 4]]) as u32;
            let width = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]) as u32;
            return Ok((width, height));
        }
        index += length;
    }
    Err("JPEG has no supported frame header".to_string())
}

fn svg_dimensions(bytes: &[u8]) -> Result<(f64, f64), String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "SVG is not valid UTF-8".to_string())?;
    let lower = text.to_ascii_lowercase();
    for forbidden in [
        "<!doctype",
        "<!entity",
        "<script",
        "<foreignobject",
        "href=\"http",
        "href='http",
        "url(http",
        "@import",
        "javascript:",
    ] {
        if lower.contains(forbidden) {
            return Err(format!("SVG contains forbidden content: {forbidden}"));
        }
    }
    if contains_svg_event_attribute(&lower) {
        return Err("SVG contains an event-handler attribute".to_string());
    }

    let mut root = text.trim_start_matches('\u{feff}').trim_start();
    if root.starts_with("<?xml") {
        let end = root
            .find("?>")
            .ok_or_else(|| "invalid SVG XML declaration".to_string())?;
        root = root[end + 2..].trim_start();
    }
    if !root.starts_with("<svg")
        || !root
            .as_bytes()
            .get(4)
            .map(|byte| byte.is_ascii_whitespace() || *byte == b'>')
            .unwrap_or(false)
    {
        return Err("SVG root element must be first".to_string());
    }
    let tag_end = root
        .find('>')
        .ok_or_else(|| "unterminated SVG root element".to_string())?;
    let root_tag = &root[..=tag_end];
    let width = svg_attribute(root_tag, "width").and_then(parse_svg_length);
    let height = svg_attribute(root_tag, "height").and_then(parse_svg_length);
    if let (Some(width), Some(height)) = (width, height) {
        return Ok((width, height));
    }
    let view_box = svg_attribute(root_tag, "viewBox")
        .ok_or_else(|| "SVG requires numeric width/height or viewBox".to_string())?;
    let values = view_box
        .split(|character: char| character.is_ascii_whitespace() || character == ',')
        .filter(|value| !value.is_empty())
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "invalid SVG viewBox".to_string())?;
    if values.len() != 4 {
        return Err("SVG viewBox must contain four numbers".to_string());
    }
    Ok((values[2], values[3]))
}

fn contains_svg_event_attribute(text: &str) -> bool {
    let bytes = text.as_bytes();
    for mut index in 0..bytes.len() {
        if !bytes[index].is_ascii_whitespace() {
            continue;
        }
        index += 1;
        if bytes.get(index..index + 2) != Some(b"on") {
            continue;
        }
        index += 2;
        let name_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_alphabetic) {
            index += 1;
        }
        if index == name_start {
            continue;
        }
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if bytes.get(index) == Some(&b'=') {
            return true;
        }
    }
    false
}

fn svg_attribute<'a>(tag: &'a str, expected: &str) -> Option<&'a str> {
    let bytes = tag.as_bytes();
    let mut index = 4usize;
    while index < bytes.len() {
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if bytes.get(index) == Some(&b'>') || bytes.get(index) == Some(&b'/') {
            break;
        }
        let name_start = index;
        while bytes
            .get(index)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b':' | b'_' | b'-'))
        {
            index += 1;
        }
        let name = tag.get(name_start..index)?;
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if bytes.get(index) != Some(&b'=') {
            while bytes
                .get(index)
                .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'>')
            {
                index += 1;
            }
            continue;
        }
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        let quote = *bytes.get(index)?;
        if quote != b'\'' && quote != b'"' {
            return None;
        }
        index += 1;
        let value_start = index;
        while bytes.get(index) != Some(&quote) {
            index += 1;
            if index >= bytes.len() {
                return None;
            }
        }
        let value = tag.get(value_start..index)?;
        index += 1;
        if name == expected {
            return Some(value);
        }
    }
    None
}

fn parse_svg_length(value: &str) -> Option<f64> {
    value
        .trim()
        .strip_suffix("px")
        .unwrap_or(value.trim())
        .parse()
        .ok()
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut chunks = bytes.chunks_exact(3);
    for chunk in &mut chunks {
        let value = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | chunk[2] as u32;
        output.push(TABLE[((value >> 18) & 0x3f) as usize] as char);
        output.push(TABLE[((value >> 12) & 0x3f) as usize] as char);
        output.push(TABLE[((value >> 6) & 0x3f) as usize] as char);
        output.push(TABLE[(value & 0x3f) as usize] as char);
    }
    match chunks.remainder() {
        [first] => {
            let value = (*first as u32) << 16;
            output.push(TABLE[((value >> 18) & 0x3f) as usize] as char);
            output.push(TABLE[((value >> 12) & 0x3f) as usize] as char);
            output.push_str("==");
        }
        [first, second] => {
            let value = ((*first as u32) << 16) | ((*second as u32) << 8);
            output.push(TABLE[((value >> 18) & 0x3f) as usize] as char);
            output.push(TABLE[((value >> 12) & 0x3f) as usize] as char);
            output.push(TABLE[((value >> 6) & 0x3f) as usize] as char);
            output.push('=');
        }
        _ => {}
    }
    output
}

fn add_mark_highlights<'a>(events: Vec<MdEvent<'a>>) -> Vec<MdEvent<'a>> {
    let mut output = Vec::with_capacity(events.len());
    for event in events {
        match event {
            MdEvent::Text(text) if text.contains("==") => {
                push_mark_highlight_events(text.as_ref(), &mut output)
            }
            _ => output.push(event),
        }
    }
    output
}

fn push_mark_highlight_events<'a>(text: &str, output: &mut Vec<MdEvent<'a>>) {
    let mut rest = text;
    while let Some(open) = rest.find("==") {
        let after_open = open + 2;
        let Some(close_relative) = rest[after_open..].find("==") else {
            break;
        };
        let close = after_open + close_relative;
        let body = &rest[after_open..close];
        if body.trim().is_empty() {
            break;
        }
        if open > 0 {
            output.push(MdEvent::Text(CowStr::Boxed(
                rest[..open].to_string().into_boxed_str(),
            )));
        }
        output.push(MdEvent::Html(CowStr::Borrowed(
            r#"<mark class="mdp-mark">"#,
        )));
        output.push(MdEvent::Text(CowStr::Boxed(
            body.to_string().into_boxed_str(),
        )));
        output.push(MdEvent::Html(CowStr::Borrowed("</mark>")));
        rest = &rest[close + 2..];
    }
    if !rest.is_empty() {
        output.push(MdEvent::Text(CowStr::Boxed(
            rest.to_string().into_boxed_str(),
        )));
    }
}

fn add_heading_ids<'a>(mut events: Vec<MdEvent<'a>>) -> Vec<MdEvent<'a>> {
    let mut seen = HashMap::new();
    for index in 0..events.len() {
        let generate_id = match &events[index] {
            MdEvent::Start(Tag::Heading { id: Some(id), .. }) => {
                register_heading_id(id.as_ref(), &mut seen);
                false
            }
            MdEvent::Start(Tag::Heading { id: None, .. }) => true,
            _ => false,
        };
        if !generate_id {
            continue;
        }
        let text = collect_heading_text(&events, index);
        let id_value = unique_heading_id(heading_slug(&text), &mut seen);
        if let MdEvent::Start(Tag::Heading { id, .. }) = &mut events[index] {
            *id = Some(CowStr::Boxed(id_value.into_boxed_str()));
        }
    }
    events
}

fn collect_heading_text(events: &[MdEvent<'_>], start: usize) -> String {
    let mut text = String::new();
    for event in events.iter().skip(start + 1) {
        match event {
            MdEvent::End(TagEnd::Heading(_)) => break,
            MdEvent::Text(value)
            | MdEvent::Code(value)
            | MdEvent::InlineMath(value)
            | MdEvent::DisplayMath(value) => text.push_str(value.as_ref()),
            MdEvent::SoftBreak | MdEvent::HardBreak => text.push(' '),
            _ => {}
        }
    }
    text
}

fn heading_slug(text: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for character in text.trim().chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() || character == '_' || character == '-' {
            slug.push(character);
            last_dash = false;
        } else if character.is_whitespace() && !slug.is_empty() && !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "section".to_string()
    } else {
        slug
    }
}

fn register_heading_id(id: &str, seen: &mut HashMap<String, usize>) {
    if !id.is_empty() {
        *seen.entry(id.to_string()).or_insert(0) += 1;
    }
}

fn unique_heading_id(base: String, seen: &mut HashMap<String, usize>) -> String {
    let count = seen.entry(base.clone()).or_insert(0);
    let id = if *count == 0 {
        base
    } else {
        format!("{base}-{count}")
    };
    *count += 1;
    id
}

fn starts_mermaid_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(info) = trimmed
        .strip_prefix("```")
        .or_else(|| trimmed.strip_prefix("~~~"))
    else {
        return false;
    };
    let info = info.trim_start();
    info == "mermaid"
        || info
            .strip_prefix("mermaid")
            .and_then(|suffix| suffix.chars().next())
            .map(|character| character.is_whitespace() || character == '{')
            .unwrap_or(false)
}

fn has_unescaped_at(text: &str, index: usize, needle: &str) -> bool {
    if !text[index..].starts_with(needle) {
        return false;
    }
    let backslashes = text[..index]
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'\\')
        .count();
    backslashes % 2 == 0
}

fn has_unescaped_pair(text: &str, open: &str, close: &str) -> bool {
    let mut position = 0;
    while let Some(relative) = text[position..].find(open) {
        let start = position + relative;
        if !has_unescaped_at(text, start, open) {
            position = start + open.len();
            continue;
        }
        let body_start = start + open.len();
        let mut search = body_start;
        while let Some(close_relative) = text[search..].find(close) {
            let close_at = search + close_relative;
            if has_unescaped_at(text, close_at, close) {
                return true;
            }
            search = close_at + close.len();
        }
        position = body_start;
    }
    false
}

fn has_inline_dollar_math(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'$' || !has_unescaped_at(text, index, "$") {
            index += 1;
            continue;
        }
        if bytes.get(index + 1).copied() == Some(b'$')
            || bytes
                .get(index + 1)
                .map(|byte| byte.is_ascii_whitespace())
                .unwrap_or(true)
        {
            index += 1;
            continue;
        }
        let mut closing = index + 1;
        while closing < bytes.len() {
            if bytes[closing] == b'$'
                && has_unescaped_at(text, closing, "$")
                && bytes
                    .get(closing.wrapping_sub(1))
                    .map(|byte| !byte.is_ascii_whitespace())
                    .unwrap_or(false)
            {
                return true;
            }
            closing += 1;
        }
        index += 1;
    }
    false
}

pub fn enhance_flags_for(md: &str) -> EnhanceFlags {
    EnhanceFlags {
        math: has_unescaped_pair(md, "$$", "$$")
            || has_unescaped_pair(md, "\\[", "\\]")
            || has_unescaped_pair(md, "\\(", "\\)")
            || has_inline_dollar_math(md),
        mermaid: md.lines().any(starts_mermaid_fence),
    }
}

pub fn build_enhancer_bootstrap(flags: EnhanceFlags, loaded: EnhanceFlags) -> Vec<String> {
    if !flags.any() {
        return Vec::new();
    }
    let mut scripts = Vec::new();
    if flags.math && !loaded.math {
        let mut script = String::from("(function(){\nif(!window.katex){\n");
        script.push_str(KATEX_JS);
        script.push_str("\n;try{window.katex=katex;}catch(e){}\n}\n");
        script.push_str("if(window.__setKatexCss)window.__setKatexCss('");
        script.push_str(&escape_js(KATEX_CSS));
        script.push_str("');\n})();");
        scripts.push(script);
    }
    if flags.mermaid && !loaded.mermaid {
        let mut script = String::with_capacity(MERMAID_JS.len() + 80);
        script.push_str(MERMAID_JS);
        script.push_str("\n;try{window.mermaid=mermaid;}catch(e){}\n");
        scripts.push(script);
    }
    scripts.push("if(window.__enhancePreview)window.__enhancePreview();".to_string());
    scripts
}

fn escape_js(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "md-preview-render-{name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp directory");
        path
    }

    #[test]
    fn snapshot_rejects_raw_html() {
        let directory = temp_dir("raw-html");
        let error = render_snapshot_markdown("<script>alert(1)</script>", &directory)
            .expect_err("raw HTML must fail");
        assert!(matches!(error, SnapshotInputError::RawHtml(_)));
        fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[test]
    fn snapshot_embeds_contained_image() {
        let directory = temp_dir("contained-image");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icon_1024.png"),
            directory.join("pixel.png"),
        )
        .expect("copy image");
        let rendered = render_snapshot_markdown("![pixel](pixel.png)", &directory)
            .expect("contained image should render");
        assert!(rendered.html.contains("data:image/png;base64,"));
        fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[test]
    fn snapshot_rejects_oversized_image_dimensions() {
        let directory = temp_dir("oversized-image");
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&(MAX_IMAGE_DIMENSION + 1).to_be_bytes());
        png.extend_from_slice(&1u32.to_be_bytes());
        fs::write(directory.join("huge.png"), png).expect("write image");
        let error = render_snapshot_markdown("![huge](huge.png)", &directory)
            .expect_err("oversized image must fail");
        assert!(error.to_string().contains("dimensions exceed"));
        fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[test]
    fn snapshot_rejects_active_svg_content() {
        let directory = temp_dir("active-svg");
        fs::write(
            directory.join("active.svg"),
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><script>alert(1)</script></svg>"#,
        )
        .expect("write image");
        let error = render_snapshot_markdown("![active](active.svg)", &directory)
            .expect_err("active SVG must fail");
        assert!(error.to_string().contains("forbidden content"));
        fs::write(
            directory.join("active.svg"),
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" onload = "alert(1)"/>"#,
        )
        .expect("write image");
        let error = render_snapshot_markdown("![active](active.svg)", &directory)
            .expect_err("SVG event handler must fail");
        assert!(error.to_string().contains("event-handler"));
        fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[test]
    fn snapshot_rejects_traversal_and_remote_images() {
        let directory = temp_dir("unsafe-image");
        assert!(render_snapshot_markdown("![secret](../secret.png)", &directory).is_err());
        assert!(
            render_snapshot_markdown("![remote](https://example.com/x.png)", &directory).is_err()
        );
        fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let directory = temp_dir("symlink-image");
        let outside = directory.with_extension("outside.png");
        fs::write(&outside, b"abc").expect("write outside image");
        symlink(&outside, directory.join("escape.png")).expect("create symlink");
        assert!(render_snapshot_markdown("![escape](escape.png)", &directory).is_err());
        fs::remove_dir_all(directory).expect("remove temp directory");
        fs::remove_file(outside).expect("remove outside image");
    }

    #[test]
    fn snapshot_page_contains_shared_assets_and_readiness_protocol() {
        let rendered = RenderedMarkdown {
            html: "<h1>hello</h1>".to_string(),
            flags: EnhanceFlags {
                math: true,
                mermaid: true,
            },
        };
        let page = build_snapshot_page(
            &rendered,
            SnapshotPageOptions {
                page: 0,
                width: 960,
                height: 720,
                scale: 1.0,
                theme: PreviewTheme::Dark,
                max_pages: MAX_RENDER_PAGES,
            },
        );
        assert!(page.contains("Content-Security-Policy"));
        assert!(page.contains("window.__mdPreviewRenderState"));
        assert!(page.contains("document.fonts"));
        assert!(page.contains("decodeImages"));
        assert!(page.contains("md-preview-ready:"));
        assert!(page.contains("securityLevel: 'strict'"));
        assert!(page.contains("trust: false"));
        assert!(page.contains(PREVIEW_DARK_CSS.trim()));
    }
}
