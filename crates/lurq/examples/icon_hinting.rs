//! The icon font drawn two ways, to see what hinting does. Each size shows one row of icons twice: from the
//! font loaded as any font is (`App::load_font`), and from the same file loaded with `App::load_hinted_font`.
//!
//! Sizes are in pixels of the display: the page divides them by the window's scale factor, so the row marked
//! 16 px is sixteen device pixels tall whatever the display's scaling.
//!
//! cargo run -p lurq --example icon_hinting --features winit,wgpu,screenshot
//!
//! With `--capture <file.png>` the window opens far off screen, saves its own frame to the file and closes when
//! the file is written.

use std::{
  sync::atomic::{AtomicU64, Ordering},
  time::Duration,
};

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::{Ctx, Interval, Timeout},
    wgpu_render::WgpuRenderEngine,
    winit_shell::WinitWindow,
  },
  components::{Column, Row, Text},
  layout::text_style::TextStyle,
  node::Element,
};

const ICONS: &[u8] = include_bytes!("../assets/lurq-icons.ttf");
const HINTED: &str = "lurq icons";
/// The same file under another family name, loaded without hinting.
const PLAIN: &str = "lurq plain";
/// home, settings, search, calendar, calendar-days, trash, mail, bell, folder, square, menu, plus, check-circle,
/// lock, file-text, copy, user, keyboard, hand, layers, external-link, circle.
const GLYPHS: &str = "\u{E03E}\u{E06F}\u{E06C}\u{E012}\u{E014}\u{E082}\u{E04B}\u{E00C}\u{E031}\u{E07A}\u{E04D}\
                      \u{E05C}\u{E018}\u{E04A}\u{E030}\u{E022}\u{E086}\u{E044}\u{E039}\u{E094}\u{E093}\u{E091}";
const SIZES: [f32; 7] = [12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 32.0];
const LABEL_WIDTH: f32 = 96.0;
const CAPTURE_AFTER: Duration = Duration::from_millis(1500);
/// How often the capture's file is looked at: the window closes once the file is written.
const FILE_CHECK_EVERY: Duration = Duration::from_millis(300);

#[derive(Clone, Debug, PartialEq, lurq::DevtoolsInspectable)]
struct Options {
  /// The file the window's frame is saved to; empty to stay open.
  capture: String,
}

struct Page {
  _capture: Option<Timeout>,
  _close_when_saved: Option<Interval>,
}

/// The font under another family name, so one file can be loaded twice: once hinted, once not.
fn renamed(font: &[u8], from: &str, to: &str) -> Vec<u8> {
  assert_eq!(
    from.len(),
    to.len(),
    "the name table keeps its layout only if the lengths match"
  );
  let wide = |name: &str| name.encode_utf16().flat_map(u16::to_be_bytes).collect::<Vec<u8>>();
  let mut font = font.to_vec();
  for (from, to) in [
    (from.as_bytes().to_vec(), to.as_bytes().to_vec()),
    (wide(from), wide(to)),
  ] {
    let mut searched = 0;
    while let Some(found) = font[searched..].windows(from.len()).position(|window| window == from) {
      let at = searched + found;
      font[at..at + to.len()].copy_from_slice(&to);
      searched = at + to.len();
    }
  }
  font
}

fn label(text: &str) -> Row {
  Row::new().width(LABEL_WIDTH).child(Text::styled(
    text,
    TextStyle {
      font_size: 12.0,
      ..TextStyle::default()
    },
  ))
}

/// One row of icons: `size` pixels of the display, from the family given.
fn icons(family: &str, size: f32, scale: f32) -> Text {
  Text::styled(
    GLYPHS,
    TextStyle {
      font_family: family.into(),
      font_size: size / scale,
      line_height: 1.0,
      ..TextStyle::default()
    },
  )
  .nowrap()
}

impl Component for Page {
  type Props = Options;

  fn create(ctx: &mut Ctx) -> Self {
    let capture = ctx.props::<Options>().capture.clone();
    if capture.is_empty() {
      return Self {
        _capture: None,
        _close_when_saved: None,
      };
    }
    let (window, path) = (ctx.window(), capture.clone());
    let save = ctx.create_timeout(CAPTURE_AFTER, move || window.screenshot(path.clone()));
    save.start();
    let (window, seen) = (ctx.window(), AtomicU64::new(0));
    let close_when_saved = ctx.create_interval(FILE_CHECK_EVERY, move || {
      let size = std::fs::metadata(&capture).map_or(0, |file| file.len());
      if size > 0 && seen.swap(size, Ordering::Relaxed) == size {
        window.close();
      }
    });
    close_when_saved.start();
    Self {
      _capture: Some(save),
      _close_when_saved: Some(close_when_saved),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let scale = ctx.window().info().scale_factor.max(0.25);
    let mut page = Column::new()
      .padding(24.0)
      .spacing(14.0)
      .background("#ffffff")
      .child(Text::new("Icon font: one file, loaded two ways"))
      .child(label(&format!("display scale {scale}")).width(400.0));
    for size in SIZES {
      page = page.child(
        Column::new()
          .spacing(4.0)
          .child(
            Row::new()
              .child(label(&format!("{size} px  load_font")))
              .child(icons(PLAIN, size, scale)),
          )
          .child(
            Row::new()
              .child(label(&format!("{size} px  hinted")))
              .child(icons(HINTED, size, scale)),
          ),
      );
    }
    page
  }
}

fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let capture = args
    .iter()
    .position(|arg| arg == "--capture")
    .map(|at| args.get(at + 1).expect("--capture takes a file").clone())
    .unwrap_or_default();
  if !capture.is_empty() && std::path::Path::new(&capture).exists() {
    std::fs::remove_file(&capture).expect("the earlier capture can be replaced");
  }
  let position = if capture.is_empty() {
    (120, 120)
  } else {
    (-12000, -12000)
  };

  let mut app = App::new();
  app.load_hinted_font(ICONS.to_vec());
  app.load_font(renamed(ICONS, HINTED, PLAIN));
  let mut tree = Tree::new();
  tree.set_render_engine_factory(|| Box::new(WgpuRenderEngine::new()));
  tree.mount_root::<Page>(&mut app, Options { capture });

  WinitWindow::new(app, tree)
    .with_size(900, 620)
    .with_position(position.0, position.1)
    .with_start_without_focus(true)
    .with_title("lurq icon hinting")
    .run();
}
