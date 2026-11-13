mod animation_demo;
mod atlas_upload_demo;
mod components_demo;
mod context_demo;
mod dnd_demo;
mod dynamic_demo;
mod engine_borders_demo;
mod engine_grid_demo;
mod engine_kit;
mod engine_presence_demo;
mod engine_sticky_demo;
mod engine_style_demo;
mod events_demo;
mod inputs_demo;
mod layout_demo;
mod markdown_demo;
mod persistent_storage_demo;
mod positioning_demo;
mod reactivity_demo;
mod scroll_demo;
mod sidebar;
mod sizing_demo;
mod style;
mod text_demo;
mod transform_demo;
mod visual_demo;

#[cfg(feature = "perf_profile")]
use std::io::Write;
use std::{
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
  },
  thread,
  time::{Duration, Instant},
};

#[cfg(target_os = "windows")]
use lurq::app::dx12_render::Dx12RenderEngine;
use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, wgpu_render::WgpuRenderEngine, winit_shell::WinitWindow},
  components::{Column, Image, Modal, Outlet, Rect, Root, Router, Row, Slider, Stack},
  core::Signal,
  images::ImageData,
  layout::{
    Alignment, StackAlignment,
    layout_kind::Justify,
    scrollbar::{ScrollBarStyle, ScrollBarVisibility},
    text_style::FontWeight,
  },
  node::{CursorIcon, Element, color::Color, dimension::Dimension},
  router::{RouterHandle, Routes},
};

use crate::{
  animation_demo::animation_content,
  dnd_demo::DndDemo,
  layout_demo::layout_content,
  positioning_demo::PositioningDemo,
  scroll_demo::scroll_content,
  sidebar::{DemoTab, sidebar},
  sizing_demo::sizing_content,
  style::DemoTheme,
};

const SIDEBAR_WIDTH: f32 = 200.0;
const DEFAULT_RENDERER: &str = "wgpu";
const DEFAULT_ROUTE: &str = "/dynamic-keyframes";

#[derive(Clone, lurq::DevtoolsInspectable)]
struct DemoProps {
  initial_route: String,
}

impl PartialEq for DemoProps {
  fn eq(&self, _other: &Self) -> bool {
    true
  }
}

#[cfg(feature = "mcp")]
static MCP_HANDLE: std::sync::OnceLock<lurq::mcp::McpHandle> = std::sync::OnceLock::new();

struct DemoApp {
  router: RouterHandle,
}

impl Component for DemoApp {
  type Props = DemoProps;

  fn create(ctx: &mut Ctx) -> Self {
    let theme = ctx.signal(DemoTheme::Dark);
    let modal_open = ctx.signal(false);
    let router = ctx.router(demo_routes(theme.clone(), modal_open.clone()));
    router.replace(&ctx.props::<DemoProps>().initial_route);
    #[cfg(feature = "mcp")]
    if let Some(handle) = MCP_HANDLE.get() {
      handle.set_navigator(router.navigator());
    }

    Self { router }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Router::mount(ctx, self.router.clone())
  }
}

fn demo_routes(theme: Signal<DemoTheme>, modal_open: Signal<bool>) -> Routes {
  let inputs_theme = theme.clone();
  let context_theme = theme.clone();

  Routes::new().layout(
    "/",
    move |ctx| demo_shell(ctx, theme.clone(), modal_open.clone()),
    move |routes| {
      routes
        .route("/", |_ctx| layout_content())
        .route("/atlas-upload", |ctx| {
          ctx.mount::<atlas_upload_demo::AtlasUploadProbe>(atlas_upload_demo::AtlasUploadProbeProps)
        })
        .route("/dynamic-keyframes", |_ctx| dynamic_demo::dynamic_keyframes_content())
        .route("/dynamic-images", |_ctx| dynamic_demo::dynamic_images_content())
        .route("/sizing", |_ctx| sizing_content())
        .route("/position", |ctx| ctx.mount::<PositioningDemo>(()))
        .route("/dnd", |ctx| ctx.mount::<DndDemo>(()))
        .route("/animation", |_ctx| animation_content())
        .route("/transform", |_ctx| transform_demo::transform_content())
        .route("/scroll", |_ctx| scroll_content())
        .route("/visual", |_ctx| visual_demo::visual_content())
        .route("/text", |_ctx| text_demo::text_content())
        .route("/markdown", |ctx| markdown_demo::markdown_content(ctx))
        .route("/persistent-storage", |ctx| {
          ctx.mount::<persistent_storage_demo::PersistentStorageDemo>(())
        })
        .route("/inputs", move |ctx| {
          ctx.mount::<inputs_demo::InputsDemo>(inputs_theme.get())
        })
        .route("/borders", |ctx| ctx.mount::<engine_borders_demo::BordersDemo>(()))
        .route("/grid", |ctx| ctx.mount::<engine_grid_demo::GridDemo>(()))
        .route("/sticky", |ctx| ctx.mount::<engine_sticky_demo::StickyDemo>(()))
        .route("/presence", |ctx| ctx.mount::<engine_presence_demo::PresenceDemo>(()))
        .route("/style", |ctx| ctx.mount::<engine_style_demo::StyleDemo>(()))
        .route("/events", |ctx| ctx.mount::<events_demo::EventsDemo>(()))
        .route("/reactivity", |ctx| ctx.mount::<reactivity_demo::ReactivityDemo>(()))
        .route("/components", |ctx| ctx.mount::<components_demo::ComponentsDemo>(()))
        .route("/context", move |ctx| {
          ctx.mount::<context_demo::ContextDemo>(context_demo::ContextDemoProps {
            theme: context_theme.clone(),
          })
        })
        .fallback(|_ctx| layout_content())
    },
  )
}

fn demo_shell(ctx: &mut Ctx, theme: Signal<DemoTheme>, modal_open: Signal<bool>) -> Element {
  let theme = theme.get();
  let palette = theme.palette();
  let selected_tab = DemoTab::from_path(&ctx.route_path());

  Row::new()
    .align_items(Alignment::Stretch)
    .child(
      lurq::components::ScrollVertical::new(sidebar(ctx, selected_tab, theme))
        .scrollbar(ScrollBarStyle {
          visible: ScrollBarVisibility::Auto,
          width: 6.0,
          thumb_color: Color::from_hex(palette.primary),
          thumb_radius: 4.0,
          ..ScrollBarStyle::default()
        })
        .scrollbar_hovered(move |style| style.with_thumb_color(Color::from_hex(palette.accent)))
        .width(SIDEBAR_WIDTH)
        .background(palette.surface_dark),
    )
    .child(
      Column::new()
        .child(demo_toolbar(selected_tab, theme, modal_open.clone()))
        .child(
          lurq::components::ScrollVertical::new(Outlet::mount(ctx))
            .scrollbar(ScrollBarStyle {
              visible: ScrollBarVisibility::Auto,
              width: 7.0,
              thumb_color: Color::from_hex(palette.primary),
              thumb_radius: 4.0,
              ..ScrollBarStyle::default()
            })
            .scrollbar_hovered(move |style| style.with_thumb_color(Color::from_hex(palette.accent)))
            .background(palette.bg)
            .flex(1.0),
        )
        .background(palette.bg)
        .flex(1.0),
    )
    .background(palette.bg)
    .child(
      Modal::new(ctx.mount::<DemoModalContent>(DemoModalProps {
        theme,
        modal_open: modal_open.clone(),
      }))
      .open(modal_open)
      .target(Root),
    )
    .into()
}

fn demo_toolbar(selected_tab: DemoTab, theme: DemoTheme, modal_open: Signal<bool>) -> Element {
  let palette = theme.palette();
  Row::new()
    .id("demo-toolbar")
    .align_items(Alignment::Center)
    .justify(Justify::End)
    .child(style::text(
      selected_tab.label(),
      13.0,
      FontWeight::Bold,
      palette.text_muted,
    ))
    .child(lurq::components::Spacer::new().flex(1.0))
    .child(demo_button("Open modal", palette.primary, move || modal_open.set(true)))
    .height(54.0)
    .padding_horizontal(18.0)
    .background(palette.surface_dark)
    .border_inside(1.0, Color::from_hex(palette.border))
    .into()
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct DemoModalProps {
  theme: DemoTheme,
  modal_open: Signal<bool>,
}

impl PartialEq for DemoModalProps {
  fn eq(&self, other: &Self) -> bool {
    self.theme == other.theme
  }
}

struct DemoModalContent {
  value: Signal<i32>,
}

impl Component for DemoModalContent {
  type Props = DemoModalProps;

  fn create(ctx: &mut Ctx) -> Self {
    Self { value: ctx.signal(64) }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<DemoModalProps>();
    let palette = props.theme.palette();
    let modal_open = props.modal_open.clone();
    let value = self.value.clone();
    let current = value.get();

    Stack::new()
      .stack_align(StackAlignment::Center)
      .child(
        Rect::new(Dimension::Pct(100.0), Dimension::Pct(100.0))
          .background("#000000")
          .opacity(0.58)
          .on_click({
            let modal_open = modal_open.clone();
            move |_| modal_open.set(false)
          }),
      )
      .child(
        Column::new()
          .spacing(12.0)
          .child(style::text("Demo modal", 22.0, FontWeight::Bold, palette.text))
          .child(
            style::text(
              "This panel is declared as a render-flow Modal and rendered above the app content.",
              13.0,
              FontWeight::Medium,
              palette.text_muted,
            )
            .width(Dimension::Pct(100.0)),
          )
          .child(
            Column::new()
              .spacing(8.0)
              .child(
                Row::new()
                  .align_items(Alignment::Center)
                  .child(style::text("Modal slider", 14.0, FontWeight::Bold, palette.text))
                  .child(lurq::components::Spacer::new().flex(1.0))
                  .child(style::text(
                    &current.to_string(),
                    12.0,
                    FontWeight::Medium,
                    palette.accent,
                  )),
              )
              .child(
                Slider::new(value)
                  .range(0, 100)
                  .height(18.0)
                  .width(Dimension::Pct(100.0))
                  .background("#cbd5e1")
                  .rounded(9.0)
                  .cursor(CursorIcon::Pointer)
                  .focused(move |style| style.border_inside(2.0, Color::from_hex(palette.primary))),
              )
              .width(Dimension::Pct(100.0)),
          )
          .child(
            Row::new()
              .justify(Justify::End)
              .child(demo_button("Close", palette.primary, move || modal_open.set(false))),
          )
          .width(420.0)
          .padding(24.0)
          .background(palette.surface)
          .border_inside(1.0, Color::from_hex(palette.border))
          .rounded(10.0),
      )
      .size(Dimension::Pct(100.0), Dimension::Pct(100.0))
  }
}

fn demo_button(label: &str, fill: &'static str, on_click: impl Fn() + Send + Sync + 'static) -> Element {
  Row::new()
    .class("demo-button")
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .child(style::text(label, 12.0, FontWeight::Bold, "#ffffff"))
    .height(34.0)
    .padding_horizontal(14.0)
    .background(fill)
    .rounded(6.0)
    .cursor(CursorIcon::Pointer)
    .hovered(|style| style.background("#60a5fa"))
    .active(|style| style.background("#2563eb"))
    .on_click(move |_| on_click())
    .into()
}

struct DemoOptions {
  renderer: String,
  initial_route: String,
  profile_log: Option<std::path::PathBuf>,
  continuous_video_probe: Option<ContinuousVideoProbeOptions>,
}

#[derive(Clone, Copy)]
struct ContinuousVideoProbeOptions {
  width: u32,
  height: u32,
  fps: u32,
}

fn set_selected_render_engine(tree: &mut Tree, selected_renderer: &str) -> String {
  let renderer = normalize_renderer_name(selected_renderer).to_owned();
  let renderer_for_factory = renderer.clone();
  tree.set_render_engine_factory(move || create_render_engine(&renderer_for_factory));
  renderer
}

fn create_render_engine(renderer: &str) -> Box<dyn lurq::app::render_engine::RenderEngine> {
  match renderer {
    "wgpu" => create_wgpu_render_engine(),
    "dx12" | "d3d12" => create_dx12_render_engine(),
    other => panic!("unknown renderer `{other}`; expected `wgpu` or `dx12`"),
  }
}

fn normalize_renderer_name(renderer: &str) -> &'static str {
  match renderer {
    "wgpu" => "wgpu",
    "dx12" | "d3d12" => "dx12",
    other => panic!("unknown renderer `{other}`; expected `wgpu` or `dx12`"),
  }
}

fn demo_options() -> DemoOptions {
  let mut renderer = DEFAULT_RENDERER.to_owned();
  let mut initial_route = DEFAULT_ROUTE.to_owned();
  let mut profile_log = None;
  let mut continuous_video_probe = None;
  let mut args = std::env::args().skip(1);
  while let Some(arg) = args.next() {
    if arg == "--renderer" {
      renderer = args
        .next()
        .unwrap_or_else(|| panic!("--renderer requires `wgpu` or `dx12`"))
        .to_ascii_lowercase();
      continue;
    }

    if let Some(renderer_arg) = arg.strip_prefix("--renderer=") {
      renderer = renderer_arg.to_ascii_lowercase();
      continue;
    }

    if arg == "--route" {
      initial_route = normalize_route(&args.next().unwrap_or_else(|| panic!("--route requires a path")));
      continue;
    }

    if let Some(route) = arg.strip_prefix("--route=") {
      initial_route = normalize_route(route);
      continue;
    }

    if arg == "--atlas-upload-probe" {
      initial_route = "/atlas-upload".to_owned();
      continue;
    }

    if arg == "--profile-log" {
      profile_log = Some(default_profile_log_path());
      continue;
    }

    if let Some(path) = arg.strip_prefix("--profile-log=") {
      profile_log = Some(std::path::PathBuf::from(path));
      continue;
    }

    if arg == "--continuous-video-probe" {
      continuous_video_probe = Some(ContinuousVideoProbeOptions {
        width: 1280,
        height: 720,
        fps: 120,
      });
      continue;
    }

    if let Some(fps) = arg.strip_prefix("--probe-fps=") {
      continuous_video_probe
        .get_or_insert(ContinuousVideoProbeOptions {
          width: 1280,
          height: 720,
          fps: 120,
        })
        .fps = fps.parse().expect("--probe-fps must be a number");
      continue;
    }

    if let Some(width) = arg.strip_prefix("--probe-width=") {
      continuous_video_probe
        .get_or_insert(ContinuousVideoProbeOptions {
          width: 1280,
          height: 720,
          fps: 120,
        })
        .width = width.parse().expect("--probe-width must be a number");
      continue;
    }

    if let Some(height) = arg.strip_prefix("--probe-height=") {
      continuous_video_probe
        .get_or_insert(ContinuousVideoProbeOptions {
          width: 1280,
          height: 720,
          fps: 120,
        })
        .height = height.parse().expect("--probe-height must be a number");
    }
  }

  DemoOptions {
    renderer,
    initial_route,
    profile_log,
    continuous_video_probe,
  }
}

fn normalize_route(route: &str) -> String {
  if route.starts_with('/') {
    route.to_owned()
  } else {
    format!("/{route}")
  }
}

fn default_profile_log_path() -> std::path::PathBuf {
  std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("../..")
    .join("target")
    .join("perf_profile.log")
}

fn default_persistent_storage_path() -> std::path::PathBuf {
  std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("../..")
    .join("target")
    .join("demo-persistent-storage.redb")
}

fn create_wgpu_render_engine() -> Box<dyn lurq::app::render_engine::RenderEngine> {
  Box::new(WgpuRenderEngine::new())
}

#[cfg(target_os = "windows")]
fn create_dx12_render_engine() -> Box<dyn lurq::app::render_engine::RenderEngine> {
  Box::new(Dx12RenderEngine::new())
}

#[cfg(not(target_os = "windows"))]
fn create_dx12_render_engine() -> Box<dyn lurq::app::render_engine::RenderEngine> {
  panic!("--renderer dx12 requires Windows");
}

fn init_tracing() {
  let filter = tracing_subscriber::EnvFilter::try_from_default_env()
    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,lurq=info,demo=info"));
  #[cfg(feature = "mcp")]
  {
    use tracing_subscriber::layer::SubscriberExt as _;
    let subscriber = tracing_subscriber::fmt()
      .with_env_filter(filter)
      .compact()
      .finish()
      .with(lurq::mcp::log_layer());
    let _ = tracing::subscriber::set_global_default(subscriber);
  }
  #[cfg(not(feature = "mcp"))]
  let _ = tracing_subscriber::fmt().with_env_filter(filter).compact().try_init();
}

struct ContinuousVideoProbeStats {
  started_at: Instant,
  paints: u32,
  rendered: u32,
  version_changes: u32,
  repeated_versions: u32,
  skipped_versions: u64,
  max_paint_delta: Duration,
  last_version: Option<u64>,
}

impl ContinuousVideoProbeStats {
  fn new(now: Instant) -> Self {
    Self {
      started_at: now,
      paints: 0,
      rendered: 0,
      version_changes: 0,
      repeated_versions: 0,
      skipped_versions: 0,
      max_paint_delta: Duration::ZERO,
      last_version: None,
    }
  }
}

fn run_continuous_video_probe(options: DemoOptions, probe: ContinuousVideoProbeOptions) {
  let app = App::new();
  let mut tree = Tree::new();
  let renderer = set_selected_render_engine(&mut tree, &options.renderer);
  let image = ImageData::streaming_rgba(
    vec![0; (probe.width * probe.height * 4) as usize],
    probe.width,
    probe.height,
  );
  let produced = Arc::new(AtomicU64::new(0));
  let stop = Arc::new(AtomicBool::new(false));
  start_continuous_video_probe_producer(image.clone(), probe, produced.clone(), stop.clone());

  tree.set_root(
    Stack::new()
      .child(
        Image::new(image.clone())
          .width(Dimension::Pct(100.0))
          .height(Dimension::Pct(100.0)),
      )
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .background("#0f172a"),
  );

  let stats = Arc::new(Mutex::new(ContinuousVideoProbeStats::new(Instant::now())));
  let stats_for_paint = stats.clone();
  let image_for_paint = image.clone();
  let produced_for_paint = produced.clone();
  let title = format!(
    "lurq continuous video probe ({renderer}) {}x{} @ {}fps",
    probe.width, probe.height, probe.fps
  );
  tracing::info!(
    target: "video::watch::lurq",
    "continuous video probe start renderer={} size={}x{} target_fps={}",
    renderer,
    probe.width,
    probe.height,
    probe.fps
  );
  WinitWindow::new(app, tree)
    .with_title(&title)
    .with_size(probe.width.min(1280), probe.height.min(720))
    .on_paint(move |_tree, delta, report| {
      let now = Instant::now();
      let version = image_for_paint.version();
      let mut stats = stats_for_paint.lock().expect("continuous video probe stats lock");
      stats.paints += 1;
      stats.rendered += u32::from(report.rendered);
      stats.max_paint_delta = stats.max_paint_delta.max(delta);
      match stats.last_version {
        Some(last_version) if version == last_version => {
          stats.repeated_versions += 1;
        }
        Some(last_version) => {
          stats.version_changes += 1;
          stats.skipped_versions += version.saturating_sub(last_version).saturating_sub(1);
        }
        None => {
          stats.version_changes += 1;
        }
      }
      stats.last_version = Some(version);
      if now.duration_since(stats.started_at) < Duration::from_secs(1) {
        return;
      }
      tracing::info!(
        target: "video::watch::lurq",
        "continuous video probe render produced={} paints={} rendered={} version_changes={} repeated_versions={} skipped_versions={} max_paint_delta_ms={:.2} last_version={}",
        produced_for_paint.load(Ordering::Relaxed),
        stats.paints,
        stats.rendered,
        stats.version_changes,
        stats.repeated_versions,
        stats.skipped_versions,
        stats.max_paint_delta.as_secs_f64() * 1000.0,
        version
      );
      *stats = ContinuousVideoProbeStats::new(now);
      stats.last_version = Some(version);
    })
    .run();
  stop.store(true, Ordering::Relaxed);
}

fn start_continuous_video_probe_producer(
  image: ImageData,
  probe: ContinuousVideoProbeOptions,
  produced: Arc<AtomicU64>,
  stop: Arc<AtomicBool>,
) {
  thread::spawn(move || {
    let interval = Duration::from_secs_f64(1.0 / probe.fps.max(1) as f64);
    let mut next_frame_at = Instant::now();
    let mut last_frame_at = None;
    let mut stats_started_at = Instant::now();
    let mut frames = 0_u32;
    let mut max_frame_delta = Duration::ZERO;
    while !stop.load(Ordering::Relaxed) {
      let now = Instant::now();
      if now < next_frame_at {
        thread::sleep((next_frame_at - now).min(Duration::from_millis(1)));
        continue;
      }
      if let Some(last) = last_frame_at {
        max_frame_delta = max_frame_delta.max(now.duration_since(last));
      }
      last_frame_at = Some(now);
      let frame = produced.fetch_add(1, Ordering::Relaxed) + 1;
      image.update_streaming_rgba(|pixels| {
        fill_continuous_video_probe_frame(pixels, probe.width, probe.height, frame);
      });
      frames += 1;
      next_frame_at += interval;
      if now.duration_since(stats_started_at) >= Duration::from_secs(1) {
        tracing::info!(
          target: "video::watch::lurq",
          "continuous video probe producer target_fps={} produced={} max_frame_delta_ms={:.2}",
          probe.fps,
          frames,
          max_frame_delta.as_secs_f64() * 1000.0
        );
        frames = 0;
        max_frame_delta = Duration::ZERO;
        stats_started_at = now;
      }
    }
  });
}

fn fill_continuous_video_probe_frame(pixels: &mut [u8], width: u32, height: u32, frame: u64) {
  let width = width as usize;
  let height = height as usize;
  if width == 0 || height == 0 {
    return;
  }
  let expected_len = width.saturating_mul(height).saturating_mul(4);
  if pixels.len() < expected_len {
    return;
  }

  let marker_width = width.min(72);
  let marker_height = height.min(72);
  let background = [8, 12, 24, 255];
  let previous_x = ((frame.saturating_sub(1) as usize * 11) % width).min(width - 1);
  let current_x = ((frame as usize * 11) % width).min(width - 1);
  let previous_y = ((frame.saturating_sub(1) as usize * 7) % height).min(height - 1);
  let current_y = ((frame as usize * 7) % height).min(height - 1);

  fill_wrapped_rect(pixels, width, height, previous_x, 0, marker_width, height, background);
  fill_wrapped_rect(pixels, width, height, 0, previous_y, width, marker_height, background);

  let vertical_color = [
    frame.wrapping_mul(17) as u8,
    frame.wrapping_mul(31).wrapping_add(80) as u8,
    245,
    255,
  ];
  let horizontal_color = [
    245,
    frame.wrapping_mul(23).wrapping_add(40) as u8,
    frame.wrapping_mul(13) as u8,
    255,
  ];
  fill_wrapped_rect(
    pixels,
    width,
    height,
    current_x,
    0,
    marker_width,
    height,
    vertical_color,
  );
  fill_wrapped_rect(
    pixels,
    width,
    height,
    0,
    current_y,
    width,
    marker_height,
    horizontal_color,
  );
}

fn fill_wrapped_rect(
  pixels: &mut [u8],
  width: usize,
  height: usize,
  x: usize,
  y: usize,
  rect_width: usize,
  rect_height: usize,
  color: [u8; 4],
) {
  for row in 0..rect_height {
    let py = (y + row) % height;
    for col in 0..rect_width {
      let px = (x + col) % width;
      let i = (py * width + px) * 4;
      pixels[i..i + 4].copy_from_slice(&color);
    }
  }
}

fn main() {
  init_tracing();
  let options = demo_options();
  if let Some(probe) = options.continuous_video_probe {
    run_continuous_video_probe(options, probe);
    return;
  }

  let mut app = App::new();
  let mut tree = Tree::new();
  app.set_resource_root(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"));
  app
    .set_persistent_storage_path(default_persistent_storage_path())
    .expect("open demo persistent storage");
  lurq::app::devtools::load_fonts(&mut app);
  let renderer = set_selected_render_engine(&mut tree, &options.renderer);
  animation_demo::register_keyframes(&mut tree);
  engine_presence_demo::register_keyframes(&mut tree);
  #[cfg(feature = "mcp")]
  {
    use lurq::mcp::{McpConfig, McpTool, Scope};
    let handle = tree.enable_mcp(
      McpConfig::new()
        .app_name("lurq-demo")
        .scopes([Scope::Observe, Scope::Interact, Scope::Navigate])
        .scope(Scope::custom("demo"))
        .tool(
          McpTool::new("demo_route")
            .description("Report the demo's current route and renderer")
            .scope(Scope::custom("demo"))
            .read_only()
            .handler(|ctx, _args| {
              Ok(serde_json::json!({
                "scale_factor": ctx.tree.scale_factor(),
                "frame_count": ctx.tree.frame_count(),
              }))
            }),
        ),
    );
    eprintln!(
      "MCP listening on http://127.0.0.1:{}/mcp (token {})",
      handle.port(),
      handle.token()
    );
    // Published before mount_root so DemoApp::create can hand over its
    // router's navigator.
    let _ = MCP_HANDLE.set(handle);
  }
  tree.mount_root::<DemoApp>(
    &mut app,
    DemoProps {
      initial_route: options.initial_route,
    },
  );
  tree.mount_devtools(&mut app);
  let title = format!("lurq demo ({renderer})");
  #[cfg_attr(not(feature = "perf_profile"), allow(unused_mut))]
  let mut window = WinitWindow::new(app, tree).with_title(&title);
  if let Some(profile_path) = options.profile_log {
    #[cfg(feature = "perf_profile")]
    {
      if let Some(parent) = profile_path.parent() {
        std::fs::create_dir_all(parent).expect("create perf profile directory");
      }
      let profile_file = std::fs::File::create(&profile_path).expect("create perf profile log");
      let mut profile_writer = std::io::BufWriter::new(profile_file);
      eprintln!("writing perf profile to {}", profile_path.display());
      window = window.on_paint(move |t, delta, report| {
        let prof = t.profile();
        writeln!(
          profile_writer,
          "Profile for frame delta={:.2}ms rendered={} layout_recalc={} {prof}",
          delta.as_secs_f64() * 1000.0,
          report.rendered,
          report.layout_recalculated
        )
        .expect("write perf profile frame");
        profile_writer.flush().expect("flush perf profile frame");
      });
    }

    #[cfg(not(feature = "perf_profile"))]
    {
      eprintln!(
        "--profile-log requires `cargo run -p demo --features perf_profile`; ignoring {}",
        profile_path.display()
      );
    }
  }
  window.run();
}
