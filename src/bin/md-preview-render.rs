use gtk::prelude::*;
use md_preview::render::{
    build_snapshot_page, render_snapshot_markdown, PreviewTheme, SnapshotPageOptions,
    MAX_MARKDOWN_BYTES, MAX_RENDER_PAGES,
};
use serde::Serialize;
use std::cell::{Cell, RefCell};
use std::convert::TryFrom;
use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;
use webkit2gtk::{
    NetworkProxyMode, NetworkProxySettings, PermissionRequestExt, SnapshotOptions, SnapshotRegion,
    WebContext, WebContextExt, WebView, WebViewExt, WebsiteDataManagerExt,
};

const MIN_DIMENSION: u32 = 64;
const MAX_DIMENSION: u32 = 4096;
const MIN_SCALE: f64 = 0.5;
const MAX_SCALE: f64 = 4.0;
const MAX_PIXEL_DIMENSION: u32 = 8192;
const MAX_OUTPUT_PIXELS: u64 = 33_554_432;
const MIN_TIMEOUT_MS: u64 = 100;
const MAX_TIMEOUT_MS: u64 = 120_000;

const EXIT_INVALID_INPUT: i32 = 2;
const EXIT_SOURCE_FAILURE: i32 = 3;
const EXIT_UNSAFE_CONTENT: i32 = 4;
const EXIT_RENDER_FAILURE: i32 = 5;
const EXIT_RENDER_TIMEOUT: i32 = 6;
const EXIT_OUTPUT_FAILURE: i32 = 7;
const EXIT_PAGE_RANGE: i32 = 8;

const HELP: &str = r#"Usage: md-preview-render [OPTIONS]

Render one screenshot-quality Markdown page tile to PNG.

Required options:
  --input <path>       UTF-8 Markdown source (maximum 8 MiB)
  --output <path>      Atomic PNG output path
  --page <index>       Zero-based page index
  --width <pixels>     Logical viewport width (64..4096)
  --height <pixels>    Logical viewport height (64..4096)
  --scale <number>     Pixel scale (0.5..4.0)
  --theme <theme>      light or dark
  --timeout-ms <ms>    Render timeout (100..120000)
  --software-rendering Disable WebKit hardware acceleration

Other options:
  -h, --help           Print this help
  --version            Print the renderer version

Success output:
  stdout contains one JSON line with schema_version 1, renderer/version,
  canonical source and byte count, page/pages, logical width/height, scale,
  physical pixel_width/pixel_height, theme, and completed total_height.
  Diagnostics are written to stderr.

Exit status:
  2 invalid arguments     3 source failure       4 unsafe content/path
  5 render failure       6 render timeout       7 output failure
  8 page out of range
"#;

#[derive(Debug)]
struct AppError {
    code: i32,
    message: String,
}

impl AppError {
    fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::new(EXIT_INVALID_INPUT, message)
    }
}

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Version,
    Render(RenderRequest),
}

#[derive(Debug, PartialEq)]
struct RenderRequest {
    input: PathBuf,
    output: PathBuf,
    page: u32,
    width: u32,
    height: u32,
    scale: f64,
    theme: PreviewTheme,
    timeout_ms: u64,
    software_rendering: bool,
    pixel_width: u32,
    pixel_height: u32,
}

struct PreparedRender {
    request: RenderRequest,
    source: PathBuf,
    source_bytes: usize,
    page: String,
    output: AtomicOutput,
}

struct AtomicOutput {
    final_path: PathBuf,
    temporary_path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl AtomicOutput {
    fn prepare(path: &Path) -> Result<Self, AppError> {
        if path.extension().and_then(|value| value.to_str()) != Some("png") {
            return Err(AppError::invalid("--output must end in .png"));
        }
        let file_name = path
            .file_name()
            .ok_or_else(|| AppError::invalid("--output must name a file"))?;
        let requested_parent = path.parent().unwrap_or_else(|| Path::new("."));
        let requested_parent = if requested_parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            requested_parent
        };
        let parent = requested_parent.canonicalize().map_err(|error| {
            AppError::new(
                EXIT_OUTPUT_FAILURE,
                format!(
                    "cannot access output directory {}: {error}",
                    requested_parent.display()
                ),
            )
        })?;
        let final_path = parent.join(file_name);
        if final_path
            .symlink_metadata()
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
        {
            return Err(AppError::new(
                EXIT_OUTPUT_FAILURE,
                format!("output path is a directory: {}", final_path.display()),
            ));
        }

        let display_name = file_name.to_string_lossy();
        for attempt in 0..100u32 {
            let temporary_path = parent.join(format!(
                ".{display_name}.md-preview-render.{}.{}.tmp",
                std::process::id(),
                attempt
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary_path)
            {
                Ok(file) => {
                    return Ok(Self {
                        final_path,
                        temporary_path,
                        file: Some(file),
                        committed: false,
                    })
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(AppError::new(
                        EXIT_OUTPUT_FAILURE,
                        format!(
                            "cannot create temporary output in {}: {error}",
                            parent.display()
                        ),
                    ))
                }
            }
        }
        Err(AppError::new(
            EXIT_OUTPUT_FAILURE,
            format!("cannot allocate temporary output in {}", parent.display()),
        ))
    }

    fn commit(mut self, surface: &cairo::ImageSurface) -> Result<PathBuf, AppError> {
        let mut file = self.file.take().ok_or_else(|| {
            AppError::new(EXIT_OUTPUT_FAILURE, "temporary output is not available")
        })?;
        surface.write_to_png(&mut file).map_err(|error| {
            AppError::new(EXIT_OUTPUT_FAILURE, format!("cannot encode PNG: {error}"))
        })?;
        file.flush().map_err(|error| {
            AppError::new(
                EXIT_OUTPUT_FAILURE,
                format!("cannot flush PNG output: {error}"),
            )
        })?;
        file.sync_all().map_err(|error| {
            AppError::new(
                EXIT_OUTPUT_FAILURE,
                format!("cannot sync PNG output: {error}"),
            )
        })?;
        drop(file);
        fs::rename(&self.temporary_path, &self.final_path).map_err(|error| {
            AppError::new(
                EXIT_OUTPUT_FAILURE,
                format!(
                    "cannot atomically replace {}: {error}",
                    self.final_path.display()
                ),
            )
        })?;
        self.committed = true;
        Ok(self.final_path.clone())
    }
}

impl Drop for AtomicOutput {
    fn drop(&mut self) {
        if !self.committed {
            self.file.take();
            let _ = fs::remove_file(&self.temporary_path);
        }
    }
}

#[derive(Debug)]
struct ReadySnapshot {
    surface: cairo::ImageSurface,
    pages: u32,
    total_height: u64,
    viewport_width: u32,
    viewport_height: u32,
}

#[derive(Debug, PartialEq, Eq)]
enum TitleEvent {
    Ignore,
    Ready {
        pages: u32,
        total_height: u64,
        viewport_width: u32,
        viewport_height: u32,
    },
    PageOutOfRange(u32),
    Failed(String),
}

#[derive(Serialize)]
struct RenderMetadata {
    schema_version: u8,
    renderer: &'static str,
    renderer_version: &'static str,
    source: String,
    source_bytes: usize,
    page: u32,
    pages: u32,
    width: u32,
    height: u32,
    scale: f64,
    pixel_width: u32,
    pixel_height: u32,
    theme: &'static str,
    total_height: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("md-preview-render: {}", error.message);
        std::process::exit(error.code);
    }
}

fn run() -> Result<(), AppError> {
    let command = parse_os_args(env::args_os().skip(1))?;
    match command {
        Command::Help => {
            print!("{HELP}");
            Ok(())
        }
        Command::Version => {
            println!("md-preview-render {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Command::Render(request) => render(request),
    }
}

fn render(request: RenderRequest) -> Result<(), AppError> {
    let prepared = prepare_render(request)?;
    if prepared.request.software_rendering {
        enable_software_rendering();
    }
    gtk::init().map_err(|error| {
        AppError::new(
            EXIT_RENDER_FAILURE,
            format!("cannot initialize GTK: {error}"),
        )
    })?;

    let snapshot = render_page(&prepared.page, &prepared.request)?;
    if snapshot.viewport_width != prepared.request.width
        || snapshot.viewport_height != prepared.request.height
    {
        return Err(AppError::new(
            EXIT_RENDER_FAILURE,
            format!(
                "logical viewport mismatch: expected {}x{}, got {}x{}",
                prepared.request.width,
                prepared.request.height,
                snapshot.viewport_width,
                snapshot.viewport_height
            ),
        ));
    }
    if snapshot.surface.width() != prepared.request.pixel_width as i32
        || snapshot.surface.height() != prepared.request.pixel_height as i32
    {
        return Err(AppError::new(
            EXIT_RENDER_FAILURE,
            format!(
                "snapshot dimensions mismatch: expected {}x{}, got {}x{}",
                prepared.request.pixel_width,
                prepared.request.pixel_height,
                snapshot.surface.width(),
                snapshot.surface.height()
            ),
        ));
    }

    let request = &prepared.request;
    let metadata = RenderMetadata {
        schema_version: 1,
        renderer: "md-preview-render",
        renderer_version: env!("CARGO_PKG_VERSION"),
        source: prepared.source.display().to_string(),
        source_bytes: prepared.source_bytes,
        page: request.page,
        pages: snapshot.pages,
        width: request.width,
        height: request.height,
        scale: request.scale,
        pixel_width: request.pixel_width,
        pixel_height: request.pixel_height,
        theme: request.theme.as_str(),
        total_height: snapshot.total_height,
    };
    let mut metadata_json = serde_json::to_vec(&metadata).map_err(|error| {
        AppError::new(
            EXIT_OUTPUT_FAILURE,
            format!("cannot serialize render metadata: {error}"),
        )
    })?;
    metadata_json.push(b'\n');
    let output = prepared.output.commit(&snapshot.surface)?;
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    stdout
        .write_all(&metadata_json)
        .and_then(|()| stdout.flush())
        .map_err(|error| {
            let _ = fs::remove_file(&output);
            AppError::new(
                EXIT_OUTPUT_FAILURE,
                format!("cannot write metadata to stdout: {error}"),
            )
        })?;
    Ok(())
}

fn prepare_render(request: RenderRequest) -> Result<PreparedRender, AppError> {
    let source = request.input.canonicalize().map_err(|error| {
        AppError::new(
            EXIT_SOURCE_FAILURE,
            format!("cannot resolve {}: {error}", request.input.display()),
        )
    })?;
    let metadata = source.metadata().map_err(|error| {
        AppError::new(
            EXIT_SOURCE_FAILURE,
            format!("cannot inspect {}: {error}", source.display()),
        )
    })?;
    if !metadata.is_file() {
        return Err(AppError::new(
            EXIT_SOURCE_FAILURE,
            format!("source is not a regular file: {}", source.display()),
        ));
    }
    if metadata.len() > MAX_MARKDOWN_BYTES as u64 {
        return Err(AppError::new(
            EXIT_SOURCE_FAILURE,
            format!("source exceeds the {} byte limit", MAX_MARKDOWN_BYTES),
        ));
    }
    let file = File::open(&source).map_err(|error| {
        AppError::new(
            EXIT_SOURCE_FAILURE,
            format!("cannot open {}: {error}", source.display()),
        )
    })?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_MARKDOWN_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            AppError::new(
                EXIT_SOURCE_FAILURE,
                format!("cannot read {}: {error}", source.display()),
            )
        })?;
    if bytes.len() > MAX_MARKDOWN_BYTES {
        return Err(AppError::new(
            EXIT_SOURCE_FAILURE,
            format!("source exceeds the {} byte limit", MAX_MARKDOWN_BYTES),
        ));
    }
    let markdown = String::from_utf8(bytes).map_err(|error| {
        AppError::new(
            EXIT_SOURCE_FAILURE,
            format!("source is not valid UTF-8: {error}"),
        )
    })?;
    let base = source
        .parent()
        .ok_or_else(|| AppError::new(EXIT_SOURCE_FAILURE, "source has no containing directory"))?;
    let rendered = render_snapshot_markdown(&markdown, base)
        .map_err(|error| AppError::new(EXIT_UNSAFE_CONTENT, format!("unsafe Markdown: {error}")))?;
    let page = build_snapshot_page(
        &rendered,
        SnapshotPageOptions {
            page: request.page,
            width: request.width,
            height: request.height,
            scale: request.scale,
            theme: request.theme,
            max_pages: MAX_RENDER_PAGES,
        },
    );
    let output = AtomicOutput::prepare(&request.output)?;
    if output.final_path == source {
        return Err(AppError::invalid(
            "--output must not replace the source file",
        ));
    }
    Ok(PreparedRender {
        request,
        source,
        source_bytes: markdown.len(),
        page,
        output,
    })
}

fn render_page(page: &str, request: &RenderRequest) -> Result<ReadySnapshot, AppError> {
    let context = WebContext::new_ephemeral();
    let mut proxy = NetworkProxySettings::new(Some("http://127.0.0.1:9"), &[]);
    let data_manager = context.website_data_manager().ok_or_else(|| {
        AppError::new(
            EXIT_RENDER_FAILURE,
            "ephemeral WebKit context has no website data manager",
        )
    })?;
    data_manager.set_network_proxy_settings(NetworkProxyMode::Custom, Some(&mut proxy));

    let main_loop = glib::MainLoop::new(None, false);
    let result: Rc<RefCell<Option<Result<ReadySnapshot, AppError>>>> = Rc::new(RefCell::new(None));
    let snapshot_started = Rc::new(Cell::new(false));

    let window = gtk::OffscreenWindow::new();
    window.set_default_size(request.pixel_width as i32, request.pixel_height as i32);
    let webview = WebView::with_context(&context);
    webview.set_size_request(request.pixel_width as i32, request.pixel_height as i32);
    webview.set_zoom_level(request.scale);
    webview.connect_permission_request(|_, permission| {
        permission.deny();
        true
    });
    window.add(&webview);

    webview.connect_load_failed({
        let main_loop = main_loop.clone();
        let result = Rc::clone(&result);
        move |_, _, uri, error| {
            finish_render(
                &result,
                &main_loop,
                Err(AppError::new(
                    EXIT_RENDER_FAILURE,
                    format!("page load failed for {uri}: {error}"),
                )),
            );
            true
        }
    });

    webview.connect_title_notify({
        let main_loop = main_loop.clone();
        let result = Rc::clone(&result);
        let snapshot_started = Rc::clone(&snapshot_started);
        move |webview| {
            let Some(title) = webview.title() else {
                return;
            };
            match parse_title(&title) {
                TitleEvent::Ignore => {}
                TitleEvent::Failed(reason) => finish_render(
                    &result,
                    &main_loop,
                    Err(AppError::new(
                        EXIT_RENDER_FAILURE,
                        format!("page readiness failed: {reason}"),
                    )),
                ),
                TitleEvent::PageOutOfRange(pages) => finish_render(
                    &result,
                    &main_loop,
                    Err(AppError::new(
                        EXIT_PAGE_RANGE,
                        format!("page is out of range; document has {pages} page(s)"),
                    )),
                ),
                TitleEvent::Ready {
                    pages,
                    total_height,
                    viewport_width,
                    viewport_height,
                } => {
                    if snapshot_started.replace(true) {
                        return;
                    }
                    webview.snapshot(
                        SnapshotRegion::Visible,
                        SnapshotOptions::NONE,
                        None::<&gio::Cancellable>,
                        {
                            let main_loop = main_loop.clone();
                            let result = Rc::clone(&result);
                            move |snapshot| {
                                let completed = snapshot
                                    .map_err(|error| {
                                        AppError::new(
                                            EXIT_RENDER_FAILURE,
                                            format!("snapshot failed: {error}"),
                                        )
                                    })
                                    .and_then(|surface| {
                                        cairo::ImageSurface::try_from(surface)
                                            .map_err(|_| {
                                                AppError::new(
                                                    EXIT_RENDER_FAILURE,
                                                    "snapshot did not return an image surface",
                                                )
                                            })
                                            .map(|surface| ReadySnapshot {
                                                surface,
                                                pages,
                                                total_height,
                                                viewport_width,
                                                viewport_height,
                                            })
                                    });
                                finish_render(&result, &main_loop, completed);
                            }
                        },
                    );
                }
            }
        }
    });

    window.show_all();
    webview.load_html(page, Some("about:blank"));
    glib::timeout_add_local_once(Duration::from_millis(request.timeout_ms), {
        let main_loop = main_loop.clone();
        let result = Rc::clone(&result);
        move || {
            finish_render(
                &result,
                &main_loop,
                Err(AppError::new(
                    EXIT_RENDER_TIMEOUT,
                    "timed out waiting for render completion",
                )),
            );
        }
    });
    main_loop.run();

    let completed = result.borrow_mut().take().unwrap_or_else(|| {
        Err(AppError::new(
            EXIT_RENDER_FAILURE,
            "renderer stopped without a result",
        ))
    });
    completed
}

fn enable_software_rendering() {
    env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
}

fn finish_render(
    state: &Rc<RefCell<Option<Result<ReadySnapshot, AppError>>>>,
    main_loop: &glib::MainLoop,
    completed: Result<ReadySnapshot, AppError>,
) {
    let mut state = state.borrow_mut();
    if state.is_none() {
        *state = Some(completed);
        main_loop.quit();
    }
}

fn parse_title(title: &str) -> TitleEvent {
    if let Some(value) = title.strip_prefix("md-preview-page-out-of-range:") {
        return value
            .parse()
            .map(TitleEvent::PageOutOfRange)
            .unwrap_or_else(|_| TitleEvent::Failed("invalid page-range metadata".to_string()));
    }
    if let Some(reason) = title.strip_prefix("md-preview-failed:") {
        return TitleEvent::Failed(reason.to_string());
    }
    let Some(value) = title.strip_prefix("md-preview-ready:") else {
        return TitleEvent::Ignore;
    };
    let fields = value.split(':').collect::<Vec<_>>();
    if fields.len() != 4 {
        return TitleEvent::Failed("invalid readiness metadata".to_string());
    }
    let parsed = (
        fields[0].parse::<u32>(),
        fields[1].parse::<u64>(),
        fields[2].parse::<u32>(),
        fields[3].parse::<u32>(),
    );
    match parsed {
        (Ok(pages), Ok(total_height), Ok(viewport_width), Ok(viewport_height)) => {
            TitleEvent::Ready {
                pages,
                total_height,
                viewport_width,
                viewport_height,
            }
        }
        _ => TitleEvent::Failed("invalid readiness metadata".to_string()),
    }
}

fn parse_os_args(args: impl IntoIterator<Item = OsString>) -> Result<Command, AppError> {
    let args = args
        .into_iter()
        .map(|value| {
            value
                .into_string()
                .map_err(|_| AppError::invalid("arguments must be valid UTF-8"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    parse_args(args)
}

fn parse_args(args: Vec<String>) -> Result<Command, AppError> {
    if args == ["--help"] || args == ["-h"] {
        return Ok(Command::Help);
    }
    if args == ["--version"] {
        return Ok(Command::Version);
    }
    if args.is_empty() {
        return Err(AppError::invalid("missing required options; try --help"));
    }

    let mut input = None;
    let mut output = None;
    let mut page = None;
    let mut width = None;
    let mut height = None;
    let mut scale = None;
    let mut theme = None;
    let mut timeout_ms = None;
    let mut software_rendering = false;
    let mut index = 0;
    while index < args.len() {
        let option = &args[index];
        if !option.starts_with("--") {
            return Err(AppError::invalid(format!(
                "unexpected positional argument: {option}"
            )));
        }
        if option == "--software-rendering" {
            if software_rendering {
                return Err(AppError::invalid(format!("duplicate option: {option}")));
            }
            software_rendering = true;
            index += 1;
            continue;
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| AppError::invalid(format!("missing value for {option}")))?;
        match option.as_str() {
            "--input" => set_once(&mut input, PathBuf::from(value), option)?,
            "--output" => set_once(&mut output, PathBuf::from(value), option)?,
            "--page" => set_once(&mut page, parse_number::<u32>(option, value)?, option)?,
            "--width" => set_once(&mut width, parse_number::<u32>(option, value)?, option)?,
            "--height" => set_once(&mut height, parse_number::<u32>(option, value)?, option)?,
            "--scale" => set_once(&mut scale, parse_number::<f64>(option, value)?, option)?,
            "--theme" => {
                let parsed = match value.as_str() {
                    "light" => PreviewTheme::Light,
                    "dark" => PreviewTheme::Dark,
                    _ => return Err(AppError::invalid("--theme must be light or dark")),
                };
                set_once(&mut theme, parsed, option)?;
            }
            "--timeout-ms" => {
                set_once(&mut timeout_ms, parse_number::<u64>(option, value)?, option)?
            }
            _ => return Err(AppError::invalid(format!("unknown option: {option}"))),
        }
        index += 2;
    }

    let width = required(width, "--width")?;
    let height = required(height, "--height")?;
    let scale = required(scale, "--scale")?;
    let timeout_ms = required(timeout_ms, "--timeout-ms")?;
    if !(MIN_DIMENSION..=MAX_DIMENSION).contains(&width) {
        return Err(AppError::invalid(format!(
            "--width must be between {MIN_DIMENSION} and {MAX_DIMENSION}"
        )));
    }
    if !(MIN_DIMENSION..=MAX_DIMENSION).contains(&height) {
        return Err(AppError::invalid(format!(
            "--height must be between {MIN_DIMENSION} and {MAX_DIMENSION}"
        )));
    }
    if !scale.is_finite() || !(MIN_SCALE..=MAX_SCALE).contains(&scale) {
        return Err(AppError::invalid(format!(
            "--scale must be between {MIN_SCALE} and {MAX_SCALE}"
        )));
    }
    if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&timeout_ms) {
        return Err(AppError::invalid(format!(
            "--timeout-ms must be between {MIN_TIMEOUT_MS} and {MAX_TIMEOUT_MS}"
        )));
    }
    let pixel_width = scaled_dimension(width, scale, "width")?;
    let pixel_height = scaled_dimension(height, scale, "height")?;
    if u64::from(pixel_width) * u64::from(pixel_height) > MAX_OUTPUT_PIXELS {
        return Err(AppError::invalid(format!(
            "scaled output exceeds the {MAX_OUTPUT_PIXELS} pixel limit"
        )));
    }

    Ok(Command::Render(RenderRequest {
        input: required(input, "--input")?,
        output: required(output, "--output")?,
        page: required(page, "--page")?,
        width,
        height,
        scale,
        theme: required(theme, "--theme")?,
        timeout_ms,
        software_rendering,
        pixel_width,
        pixel_height,
    }))
}

fn parse_number<T>(option: &str, value: &str) -> Result<T, AppError>
where
    T: std::str::FromStr,
{
    value
        .parse::<T>()
        .map_err(|_| AppError::invalid(format!("invalid value for {option}: {value}")))
}

fn set_once<T>(slot: &mut Option<T>, value: T, option: &str) -> Result<(), AppError> {
    if slot.replace(value).is_some() {
        return Err(AppError::invalid(format!("duplicate option: {option}")));
    }
    Ok(())
}

fn required<T>(value: Option<T>, option: &str) -> Result<T, AppError> {
    value.ok_or_else(|| AppError::invalid(format!("missing required option: {option}")))
}

fn scaled_dimension(logical: u32, scale: f64, name: &str) -> Result<u32, AppError> {
    let scaled = (f64::from(logical) * scale).round();
    if scaled < 1.0 || scaled > f64::from(MAX_PIXEL_DIMENSION) {
        return Err(AppError::invalid(format!(
            "scaled {name} must not exceed {MAX_PIXEL_DIMENSION} pixels"
        )));
    }
    Ok(scaled as u32)
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
        let path = env::temp_dir().join(format!(
            "md-preview-render-cli-{name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp directory");
        path
    }

    fn complete_args() -> Vec<String> {
        [
            "--input",
            "doc.md",
            "--output",
            "tile.png",
            "--page",
            "2",
            "--width",
            "960",
            "--height",
            "540",
            "--scale",
            "2",
            "--theme",
            "dark",
            "--timeout-ms",
            "20000",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }

    #[test]
    fn parses_complete_render_request() {
        let Command::Render(request) = parse_args(complete_args()).expect("valid request") else {
            panic!("expected render request");
        };
        assert_eq!(request.page, 2);
        assert_eq!(request.pixel_width, 1920);
        assert_eq!(request.pixel_height, 1080);
        assert_eq!(request.theme, PreviewTheme::Dark);
        assert!(!request.software_rendering);
    }

    #[test]
    fn parses_software_rendering_flag() {
        let mut args = complete_args();
        args.push("--software-rendering".to_string());
        let Command::Render(request) = parse_args(args).expect("valid request") else {
            panic!("expected render request");
        };
        assert!(request.software_rendering);
    }

    #[test]
    fn rejects_duplicate_and_out_of_bounds_options() {
        let mut duplicate = complete_args();
        duplicate.extend(["--page".to_string(), "3".to_string()]);
        assert!(parse_args(duplicate)
            .expect_err("duplicate must fail")
            .message
            .contains("duplicate option"));

        let mut too_wide = complete_args();
        let width = too_wide
            .iter()
            .position(|value| value == "--width")
            .expect("width option");
        too_wide[width + 1] = "8192".to_string();
        assert!(parse_args(too_wide).is_err());
    }

    #[test]
    fn parses_readiness_titles() {
        assert_eq!(
            parse_title("md-preview-ready:4:1880:960:540"),
            TitleEvent::Ready {
                pages: 4,
                total_height: 1880,
                viewport_width: 960,
                viewport_height: 540,
            }
        );
        assert_eq!(
            parse_title("md-preview-page-out-of-range:4"),
            TitleEvent::PageOutOfRange(4)
        );
        assert_eq!(
            parse_title("md-preview-failed:document exceeds page limit"),
            TitleEvent::Failed("document exceeds page limit".to_string())
        );
    }

    #[test]
    fn rejects_oversized_source_before_rendering() {
        let directory = temp_dir("oversized-source");
        let source = directory.join("large.md");
        let file = File::create(&source).expect("create source");
        file.set_len(MAX_MARKDOWN_BYTES as u64 + 1)
            .expect("extend source");
        let request = RenderRequest {
            input: source,
            output: directory.join("tile.png"),
            page: 0,
            width: 640,
            height: 360,
            scale: 1.0,
            theme: PreviewTheme::Light,
            timeout_ms: 20_000,
            software_rendering: false,
            pixel_width: 640,
            pixel_height: 360,
        };
        let error = match prepare_render(request) {
            Ok(_) => panic!("oversized source must fail"),
            Err(error) => error,
        };
        assert_eq!(error.code, EXIT_SOURCE_FAILURE);
        assert!(!directory.join("tile.png").exists());
        fs::remove_dir_all(directory).expect("remove temp directory");
    }
}
