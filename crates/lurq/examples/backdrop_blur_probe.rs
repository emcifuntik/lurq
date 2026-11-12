//! Measures the frames of a window of frosted glass on both native backends:
//! a gradient, three glass panels over most of it with lists inside, and one
//! small glass menu over a list. Each run is a hidden Win32 window driven
//! through `Tree::pass`, with a profiling recording for the CPU phases and
//! the GPU time (wgpu's timestamp queries; the native DX12 renderer has none).
//!
//! cargo run -p lurq --release --example backdrop_blur_probe --features screenshot,wgpu,dx12,perf_profile
//!
//! Options: `--backend wgpu,dx12`, `--radius 8,32,64` (0 is the same window
//! without blur), `--scale 1,1.5`, `--scenario still,small,all,resize`,
//! `--frames 240`, `--runs 3`,
//! `--csv <file>`, `--capture <directory>` (also saves each configuration's
//! first frame and a later unchanged one as PNG), and `--compare <a> <b>`,
//! which only compares the PNGs of two such directories.
//!
//! Scenarios, a change before every frame:
//! - `still`: a row inside the last panel changes colour, as a hover does.
//!   Nothing painted behind any glass changes.
//! - `small`: a chip under the menu changes colour: behind the menu only.
//! - `all`: the gradient changes: behind every glass.
//! - `resize`: the window alternates between two sizes.
//!
//! Each number is the median over a run's frames; the table gives the median
//! of the runs and, in brackets, their lowest and highest. `cpu` is the pass
//! without the time it waited for a frame slot or in the present call,
//! `blurred` and `reused` the backdrop blurs a frame ran and the ones it put
//! back from a kept result, `tex` the layer pool's textures and `kept` the
//! part of them that holds kept results.
#[cfg(windows)]
fn main() {
  use std::{
    cell::RefCell,
    fmt::Write as _,
    num::NonZeroIsize,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, mpsc},
    time::Duration,
  };

  use lurq::{
    app::{
      App, Tree,
      profiler::{LayerProfile, PassSample, RecordingOptions, RenderProfile, SampleData},
      render_engine::{CapturedFrame, RenderCaptureTarget, RenderEngine, RenderFrameCapture},
    },
    components::{Column, Rect, Row, Stack, Text},
    layout::render_list::RenderList,
    node::{Element, Gradient},
  };
  use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, Win32WindowHandle, WindowHandle,
  };
  use windows::{
    Win32::{
      Foundation::{HWND, LPARAM, LRESULT, WPARAM},
      UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, WINDOW_EX_STYLE, WNDCLASSW, WS_POPUP,
      },
    },
    core::w,
  };

  /// The window in logical pixels.
  const WIDTH: f32 = 1440.0;
  const HEIGHT: f32 = 900.0;
  /// Frames rendered and not measured before each scenario.
  const WARM_UP: usize = 30;
  /// Frames rendered after a scenario, for its last GPU times to arrive.
  const FLUSH: usize = 12;

  extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
  }
  struct Surface(HWND);
  impl HasWindowHandle for Surface {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
      let raw = Win32WindowHandle::new(NonZeroIsize::new(self.0.0 as isize).unwrap());
      Ok(unsafe { WindowHandle::borrow_raw(raw.into()) })
    }
  }
  impl HasDisplayHandle for Surface {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
      Ok(DisplayHandle::windows())
    }
  }
  impl Drop for Surface {
    fn drop(&mut self) {
      unsafe {
        let _ = DestroyWindow(self.0);
      }
    }
  }

  /// What the probe reads from the backend after each frame, and the capture
  /// it asks the next frame for.
  #[derive(Default)]
  struct Shared {
    frames: Vec<(RenderProfile, LayerProfile)>,
    capture: Option<RenderCaptureTarget>,
    size: (u32, u32),
  }

  struct Probe {
    backend: Box<dyn RenderEngine>,
    shared: Rc<RefCell<Shared>>,
  }
  impl RenderEngine for Probe {
    fn profile_backend(&self) -> &'static str {
      self.backend.profile_backend()
    }
    fn set_profile_context(&mut self, context: lurq::app::profiler::ProfileContext) {
      self.backend.set_profile_context(context);
    }
    fn resize(&mut self, width: u32, height: u32) {
      self.shared.borrow_mut().size = (width, height);
      self.backend.resize(width, height);
    }
    fn render(&mut self, list: &RenderList, window: WindowHandle<'_>, display: DisplayHandle<'_>) -> bool {
      self.render_with_capture(list, window, display, None)
    }
    fn supports_frame_capture(&self) -> bool {
      self.backend.supports_frame_capture()
    }
    fn render_with_capture(
      &mut self,
      list: &RenderList,
      window: WindowHandle<'_>,
      display: DisplayHandle<'_>,
      capture: Option<RenderFrameCapture>,
    ) -> bool {
      let (target, (width, height)) = {
        let mut shared = self.shared.borrow_mut();
        (shared.capture.take(), shared.size)
      };
      let capture = capture.or(target.map(|target| RenderFrameCapture {
        x: 0,
        y: 0,
        width,
        height,
        target,
        window_clip: None,
      }));
      let rendered = self.backend.render_with_capture(list, window, display, capture);
      if rendered {
        self.shared.borrow_mut().frames.push((
          self.backend.last_profile().unwrap_or_default(),
          self.backend.layer_profile().unwrap_or_default(),
        ));
      }
      rendered
    }
    fn release_window_surface(&mut self) {
      self.backend.release_window_surface();
    }
    fn wants_redraw(&self) -> bool {
      self.backend.wants_redraw()
    }
    fn last_profile(&self) -> Option<RenderProfile> {
      self.backend.last_profile()
    }
    fn layer_profile(&self) -> Option<LayerProfile> {
      self.backend.layer_profile()
    }
  }

  fn gradient(step: usize) -> Gradient {
    match step % 2 {
      0 => Gradient::linear(135.0, ["#1d4ed8", "#7c3aed", "#db2777", "#f59e0b"]),
      _ => Gradient::linear(135.0, ["#1e40af", "#6d28d9", "#be185d", "#d97706"]),
    }
  }

  const ROW_FILLS: [&str; 2] = ["#ffffff14", "#ffffff3d"];
  const CHIP_FILLS: [&str; 2] = ["#22d3ee", "#f97316"];

  /// A list row: a dot, a label and a trailing bar.
  fn row(index: usize, id: Option<&'static str>) -> Element {
    let dots = ["#34d399", "#fbbf24", "#f87171", "#60a5fa"];
    let row = Row::new()
      .height(30.0)
      .padding(6.0)
      .spacing(8.0)
      .rounded(8.0)
      .background(ROW_FILLS[0])
      .child(Rect::new(10.0, 10.0).background(dots[index % dots.len()]).rounded(5.0))
      .child(Text::new(&format!("Row {index:02} of the list")).color("#f8fafc"))
      .child(Rect::new(36.0 + (index % 5) as f32 * 14.0, 6.0).background("#ffffff59").rounded(3.0));
    match id {
      Some(id) => row.id(id).into(),
      None => row.into(),
    }
  }

  /// A glass panel at a fixed place with `rows` rows; `marked` is the index of
  /// the row that gets `id`.
  fn panel(x: f32, width: f32, rows: usize, radius: f32, marked: Option<(usize, &'static str)>) -> Element {
    let mut list = Column::new()
      .padding(14.0)
      .spacing(6.0)
      .background("#ffffff24")
      .border_inside(1.0, "#ffffff4d")
      .rounded(18.0)
      .backdrop_blur(radius)
      .child(Text::new("Panel").color("#ffffff"));
    for index in 0..rows {
      let id = marked.and_then(|(at, id)| (at == index).then_some(id));
      list = list.child(row(index, id));
    }
    list.absolute(x, 16.0, width, HEIGHT - 32.0).into()
  }

  fn scene(radius: f32) -> Element {
    // The middle panel's rows are busy content under the menu; the chip in
    // one of them is what `small` changes.
    let mut content = Column::new()
      .padding(14.0)
      .spacing(6.0)
      .background("#ffffff24")
      .border_inside(1.0, "#ffffff4d")
      .rounded(18.0)
      .backdrop_blur(radius)
      .child(Text::new("Content").color("#ffffff"));
    for index in 0..22 {
      content = content.child(
        Row::new()
          .height(30.0)
          .padding(6.0)
          .spacing(8.0)
          .rounded(8.0)
          .background(if index % 2 == 0 { "#0f172a66" } else { "#f8fafc33" })
          .child(Rect::new(18.0, 18.0).background("#fde68a").rounded(9.0))
          .child(Text::new(&format!("Message {index:02}: the quick brown fox jumps over the lazy dog")).color("#f8fafc"))
          .child(if index == 10 {
            Element::from(Rect::new(120.0, 16.0).background(CHIP_FILLS[0]).rounded(8.0).id("chip"))
          } else {
            Element::from(Rect::new(40.0, 6.0).background("#ffffff59").rounded(3.0))
          }),
      );
    }
    let menu = Column::new()
      .padding(10.0)
      .spacing(4.0)
      .background("#ffffff33")
      .border_inside(1.0, "#ffffff80")
      .rounded(12.0)
      .backdrop_blur(radius)
      .child(Text::new("Open").color("#0f172a"))
      .child(Text::new("Rename").color("#0f172a"))
      .child(Text::new("Duplicate").color("#0f172a"))
      .child(Text::new("Delete").color("#7f1d1d"));
    Stack::new()
      .id("backdrop")
      .size(WIDTH, HEIGHT)
      .background_gradient(gradient(0))
      .child(panel(16.0, 300.0, 24, radius, None))
      .child(content.absolute(332.0, 16.0, 780.0, HEIGHT - 32.0))
      // The last panel: what changes inside it is painted after every other
      // panel and lies far from the menu.
      .child(panel(1128.0, 296.0, 24, radius, Some((6, "hover-row"))))
      .child(menu.absolute(600.0, 330.0, 220.0, 150.0))
      .into()
  }

  #[derive(Clone, Copy, PartialEq)]
  enum Scenario {
    Still,
    Small,
    All,
    Resize,
  }
  impl Scenario {
    fn name(self) -> &'static str {
      match self {
        Self::Still => "still",
        Self::Small => "small",
        Self::All => "all",
        Self::Resize => "resize",
      }
    }
    fn parse(name: &str) -> Self {
      match name {
        "still" => Self::Still,
        "small" => Self::Small,
        "all" => Self::All,
        "resize" => Self::Resize,
        other => panic!("unknown scenario {other}"),
      }
    }
  }

  /// The medians of one run's frames, in the order of `COLUMNS`.
  type Medians = Vec<f64>;
  const COLUMNS: [&str; 19] = [
    "cpu_us",
    "layout_us",
    "quads_us",
    "list_us",
    "encode_us",
    "submit_us",
    "present_us",
    "wait_us",
    "gpu_us",
    "draw_calls",
    "passes",
    "copies",
    "blur_draws",
    "blurred",
    "textures",
    "texture_mib",
    "gpu_frames",
    "reused",
    "kept_mib",
  ];

  fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
      return f64::NAN;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
      values[middle]
    } else {
      (values[middle - 1] + values[middle]) * 0.5
    }
  }

  fn us(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1.0e6
  }

  struct Window {
    surface: Surface,
    app: App,
    tree: Tree,
    shared: Rc<RefCell<Shared>>,
    size: (u32, u32),
    /// Counts the frames, so every frame's change differs from the last one's.
    step: usize,
  }

  impl Window {
    fn new(backend: &'static str, scale: f32, radius: f32) -> Self {
      let size = ((WIDTH * scale).round() as u32, (HEIGHT * scale).round() as u32);
      let surface = Surface(unsafe {
        let class_name = w!("lurq_backdrop_blur_probe");
        // Registered once; later calls fail and are ignored.
        RegisterClassW(&WNDCLASSW {
          lpfnWndProc: Some(wndproc),
          lpszClassName: class_name,
          ..Default::default()
        });
        CreateWindowExW(
          WINDOW_EX_STYLE::default(),
          class_name,
          w!("Backdrop blur probe"),
          WS_POPUP,
          0,
          0,
          size.0 as i32,
          size.1 as i32,
          None,
          None,
          None,
          None,
        )
        .unwrap()
      });
      let shared = Rc::new(RefCell::new(Shared::default()));
      let mut tree = Tree::new();
      let engine_shared = shared.clone();
      tree.set_render_engine_factory(move || {
        let backend: Box<dyn RenderEngine> = match backend {
          "wgpu" => Box::new(lurq::app::wgpu_render::WgpuRenderEngine::new()),
          _ => Box::new(lurq::app::dx12_render::Dx12RenderEngine::new()),
        };
        Box::new(Probe {
          backend,
          shared: engine_shared.clone(),
        })
      });
      tree.set_scale_factor(scale);
      tree.resize(size.0, size.1);
      tree.set_root(scene(radius));
      Self {
        surface,
        app: App::new(),
        tree,
        shared,
        size,
        step: 0,
      }
    }

    /// Renders one frame, after `scenario`'s change; unchanged without one.
    fn frame(&mut self, scenario: Option<Scenario>) {
      self.step += 1;
      let step = self.step;
      match scenario {
        Some(Scenario::Still) => {
          let mut row = self.tree.get_element_by_id_mut("hover-row").expect("hover-row");
          row.set_background(ROW_FILLS[step % 2]);
        }
        Some(Scenario::Small) => {
          let mut chip = self.tree.get_element_by_id_mut("chip").expect("chip");
          chip.set_background(CHIP_FILLS[step % 2]);
        }
        Some(Scenario::All) => {
          let mut backdrop = self.tree.get_element_by_id_mut("backdrop").expect("backdrop");
          backdrop.set_background_gradient(gradient(step));
        }
        Some(Scenario::Resize) => {
          // The other of the two sizes than the window has now.
          let shrink = if self.shared.borrow().size == self.size { 48 } else { 0 };
          self.tree.resize(self.size.0 - shrink, self.size.1 - shrink);
        }
        None => self.tree.request_redraw(),
      }
      let report = self.tree.pass(&mut self.app, &self.surface);
      assert!(
        report.rendered,
        "no frame at step {step} of {:?}: required {}, reasons {:?}",
        scenario.map(Scenario::name),
        report.required,
        report.reasons.names().collect::<Vec<_>>()
      );
    }

    /// Captures the next frame, rendered unchanged.
    fn capture(&mut self) -> CapturedFrame {
      let (sender, receiver) = mpsc::channel();
      self.shared.borrow_mut().capture = Some(RenderCaptureTarget::Bytes(Arc::new(move |frame| {
        let _ = sender.send(frame);
      })));
      self.frame(None);
      receiver
        .recv_timeout(Duration::from_secs(15))
        .expect("capture")
        .expect("capture ok")
    }

    /// One run of `scenario`: the medians of `frames` frames.
    fn measure(&mut self, scenario: Scenario, frames: usize) -> Medians {
      for _ in 0..WARM_UP {
        self.frame(Some(scenario));
      }
      let profiling = self.tree.profiling_handle();
      let session = profiling.start_recording(RecordingOptions::default()).expect("recording").id;
      self.shared.borrow_mut().frames.clear();
      for _ in 0..frames {
        self.frame(Some(scenario));
      }
      let measured: Vec<(RenderProfile, LayerProfile)> = std::mem::take(&mut self.shared.borrow_mut().frames);
      // GPU times arrive a few frames late.
      for _ in 0..FLUSH {
        std::thread::sleep(Duration::from_millis(4));
        self.frame(Some(scenario));
      }
      let report = profiling.end(session).expect("report");
      let passes: Vec<&PassSample> = report
        .samples
        .iter()
        .filter_map(|sample| match &sample.data {
          SampleData::Pass(pass) if pass.rendered => Some(pass),
          _ => None,
        })
        .take(frames)
        .collect();
      assert_eq!(passes.len(), frames, "a recorded pass per measured frame");
      assert_eq!(measured.len(), frames, "a render profile per measured frame");

      let column = |value: &dyn Fn(&PassSample, &RenderProfile, &LayerProfile) -> Option<f64>| {
        let mut values: Vec<f64> = passes
          .iter()
          .zip(&measured)
          .filter_map(|(pass, (render, layers))| value(pass, render, layers))
          .collect();
        median(&mut values)
      };
      let frame_profile = |pass: &PassSample| pass.frame.clone().unwrap_or_default();
      vec![
        column(&|pass, render, _| Some(us(pass.total.saturating_sub(render.acquire + render.present)))),
        column(&|pass, _, _| Some(us(pass.layout_update))),
        column(&|pass, _, _| Some(us(frame_profile(pass).quad_resolve))),
        column(&|pass, _, _| Some(us(frame_profile(pass).glyph_rasterize))),
        column(&|_, render, _| Some(us(render.encode))),
        column(&|_, render, _| Some(us(render.submit))),
        column(&|_, render, _| Some(us(render.present))),
        column(&|_, render, _| Some(us(render.acquire))),
        column(&|pass, _, _| pass.gpu.map(|gpu| us(gpu.ui))),
        column(&|_, render, _| Some(render.draw_calls as f64)),
        column(&|_, _, layers| Some(layers.render_passes as f64)),
        column(&|_, _, layers| Some(layers.target_copies as f64)),
        column(&|_, _, layers| Some(layers.blur_draws as f64)),
        column(&|_, _, layers| Some(layers.backdrops_blurred as f64)),
        column(&|_, _, layers| Some(layers.textures as f64)),
        column(&|_, _, layers| Some(layers.texture_bytes as f64 / (1024.0 * 1024.0))),
        passes.iter().filter(|pass| pass.gpu.is_some()).count() as f64,
        column(&|_, _, layers| Some(layers.backdrops_reused as f64)),
        column(&|_, _, layers| Some(layers.kept_texture_bytes as f64 / (1024.0 * 1024.0))),
      ]
    }
  }

  fn save(directory: &Path, name: &str, frame: &CapturedFrame) {
    let png = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone()).unwrap();
    png.save(directory.join(name)).unwrap();
  }

  /// Compares the PNGs of `a` with those of the same name in `b`.
  fn compare(a: &Path, b: &Path) {
    let mut names: Vec<_> = std::fs::read_dir(a)
      .expect("first directory")
      .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
      .filter(|name| name.to_string_lossy().ends_with(".png"))
      .collect();
    names.sort();
    let mut worst_of_all = 0u8;
    for name in names {
      let (Ok(first), Ok(second)) = (image::open(a.join(&name)), image::open(b.join(&name))) else {
        println!("{}: missing in the second directory", name.to_string_lossy());
        continue;
      };
      let (first, second) = (first.to_rgba8(), second.to_rgba8());
      assert_eq!(first.dimensions(), second.dimensions(), "{name:?}: sizes differ");
      let (mut worst, mut over_one, mut over_two, mut sum) = (0u8, 0usize, 0usize, 0u64);
      for (p, q) in first.pixels().zip(second.pixels()) {
        let diff = (0..3).map(|channel| p.0[channel].abs_diff(q.0[channel])).max().unwrap();
        worst = worst.max(diff);
        over_one += usize::from(diff > 1);
        over_two += usize::from(diff > 2);
        sum += u64::from(diff);
      }
      let pixels = (first.width() * first.height()) as f64;
      println!(
        "{}: worst channel difference {worst}, mean {:.4}, {over_one} pixels differ by more than 1, {over_two} by more than 2",
        name.to_string_lossy(),
        sum as f64 / pixels
      );
      worst_of_all = worst_of_all.max(worst);
    }
    println!("worst of all: {worst_of_all}");
  }

  let arguments: Vec<String> = std::env::args().skip(1).collect();
  let option = |name: &str| {
    arguments
      .iter()
      .position(|argument| argument == name)
      .and_then(|at| arguments.get(at + 1))
      .cloned()
  };
  if let Some(at) = arguments.iter().position(|argument| argument == "--compare") {
    compare(
      Path::new(arguments.get(at + 1).expect("--compare <a> <b>")),
      Path::new(arguments.get(at + 2).expect("--compare <a> <b>")),
    );
    return;
  }
  let list = |name: &str, default: &str| -> Vec<String> {
    option(name)
      .unwrap_or_else(|| default.to_owned())
      .split(',')
      .map(str::to_owned)
      .collect()
  };
  let backends: Vec<&'static str> = list("--backend", "wgpu,dx12")
    .iter()
    .map(|name| match name.as_str() {
      "wgpu" => "wgpu",
      "dx12" => "dx12",
      other => panic!("unknown backend {other}"),
    })
    .collect();
  let radii: Vec<f32> = list("--radius", "8,32,64")
    .iter()
    .map(|value| value.parse().expect("a radius"))
    .collect();
  let scales: Vec<f32> = list("--scale", "1,1.5")
    .iter()
    .map(|value| value.parse().expect("a scale"))
    .collect();
  let scenarios: Vec<Scenario> = list("--scenario", "still,small,all,resize")
    .iter()
    .map(|name| Scenario::parse(name))
    .collect();
  let frames: usize = option("--frames").map_or(240, |value| value.parse().expect("frames"));
  let runs: usize = option("--runs").map_or(3, |value| value.parse().expect("runs"));
  let capture_directory = option("--capture").map(PathBuf::from);
  if let Some(directory) = &capture_directory {
    std::fs::create_dir_all(directory).unwrap();
  }

  let mut csv = String::from("backend,scale,radius,scenario,runs,frames");
  for column in COLUMNS {
    write!(csv, ",{column},{column}_low,{column}_high").unwrap();
  }
  csv.push('\n');
  println!(
    "{:<5} {:>5} {:>6} {:<7} {:>20} {:>20} {:>20} {:>20} {:>6} {:>6} {:>6} {:>6} {:>7} {:>6} {:>8} {:>8}",
    "back",
    "scale",
    "radius",
    "frame",
    "cpu us",
    "encode us",
    "submit us",
    "gpu us",
    "draws",
    "passes",
    "copies",
    "blurs",
    "blurred",
    "reused",
    "tex MiB",
    "kept MiB"
  );
  for backend in &backends {
    for &scale in &scales {
      for &radius in &radii {
        // One window per run, so a run does not start from the last one's pools.
        let mut results: Vec<Vec<Medians>> = vec![Vec::new(); scenarios.len()];
        for run in 0..runs {
          let mut window = Window::new(backend, scale, radius);
          if run == 0
            && let Some(directory) = &capture_directory
          {
            let name = format!("{backend}-scale{scale}-radius{radius}");
            save(directory, &format!("{name}-first.png"), &window.capture());
            for _ in 0..3 {
              window.frame(None);
            }
            save(directory, &format!("{name}-later.png"), &window.capture());
          }
          for (index, scenario) in scenarios.iter().enumerate() {
            results[index].push(window.measure(*scenario, frames));
          }
        }
        for (scenario, rows) in scenarios.iter().zip(&results) {
          let spread = |column: usize| {
            let mut values: Vec<f64> = rows.iter().map(|row| row[column]).collect();
            let middle = median(&mut values);
            (middle, values[0], values[values.len() - 1])
          };
          write!(csv, "{backend},{scale},{radius},{},{runs},{frames}", scenario.name()).unwrap();
          for column in 0..COLUMNS.len() {
            let (middle, low, high) = spread(column);
            write!(csv, ",{middle:.3},{low:.3},{high:.3}").unwrap();
          }
          csv.push('\n');
          let timed = |column: usize| {
            let (middle, low, high) = spread(column);
            if middle.is_nan() {
              "-".to_owned()
            } else {
              format!("{middle:.1} [{low:.1}..{high:.1}]")
            }
          };
          let count = |column: usize| format!("{:.0}", spread(column).0);
          println!(
            "{:<5} {:>5} {:>6} {:<7} {:>20} {:>20} {:>20} {:>20} {:>6} {:>6} {:>6} {:>6} {:>7} {:>6} {:>8.1} {:>8.1}",
            backend,
            scale,
            radius,
            scenario.name(),
            timed(0),
            timed(4),
            timed(5),
            timed(8),
            count(9),
            count(10),
            count(11),
            count(12),
            count(13),
            count(17),
            spread(15).0,
            spread(18).0
          );
        }
      }
    }
  }
  if let Some(path) = option("--csv") {
    std::fs::write(&path, csv).unwrap();
    println!("wrote {path}");
  }
}

#[cfg(not(windows))]
fn main() {
  eprintln!("This probe uses a Win32 surface and the DX12 renderer.");
}
