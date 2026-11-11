//! The app lurq's iOS support is checked with: a list of screens, each for one thing a phone does differently
//! from a desktop, showing what the tree sees. It runs in a desktop window too:
//!
//! ```text
//! cargo run -p mobile_demo
//! ```
//!
//! `ios/MobileDemo.xcodeproj` wraps it in an app bundle for a simulator or a phone: its only build phase runs
//! `ios/build-rust.sh`, which builds this binary with cargo, and Xcode signs the result.

mod gestures;
mod home;
mod scroll;
mod touch;
mod ui;

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::Ctx,
    events::{HistoryDirection, HistorySwipeEvent},
    wgpu_render::WgpuRenderEngine,
    window::SafeAreaInsets,
    winit_shell::WinitWindow,
  },
  components::{Column, Outlet, Router, Row, Spacer},
  layout::{Alignment, layout_kind::Justify, text_style::FontWeight},
  node::{Element, color::Color},
  router::{RouterHandle, Routes},
};

use crate::ui::{BG, BORDER, FILL, HEADER_HEIGHT, PRIMARY, SURFACE, TAP_TARGET, TEXT, text};

struct MobileDemo {
  router: RouterHandle,
}

impl Component for MobileDemo {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      router: ctx.router(routes()),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Router::mount(ctx, self.router.clone())
  }
}

fn routes() -> Routes {
  Routes::new().layout("/", shell, |routes| {
    routes
      .route("/", home::home)
      .route("/touch", |ctx| ctx.mount::<touch::TouchScreen>(()))
      .route("/gestures", |ctx| ctx.mount::<gestures::GesturesScreen>(()))
      .route("/scroll", |ctx| ctx.mount::<scroll::ScrollScreen>(()))
      .fallback(home::home)
  })
}

/// The header, with the way back, over the screen. Backgrounds reach the edges of the display; what is read
/// and touched stays inside the safe area, clear of the status bar, the camera housing and the home indicator.
fn shell(ctx: &mut Ctx) -> Element {
  let path = ctx.route_path();
  let safe_area = ctx.window().safe_area;
  let navigator = ctx.navigator();
  Column::new()
    // A finger in from the left edge, or a trackpad's swipe, goes back as the header's arrow does; from the
    // right edge it goes forward again.
    .on_history_swipe(move |event: HistorySwipeEvent| {
      let Some(navigator) = &navigator else {
        return;
      };
      match event.direction {
        HistoryDirection::Back => navigator.back(),
        HistoryDirection::Forward => navigator.forward(),
      };
      event.stop_propagation();
    })
    .child(header(ctx, &path, safe_area))
    .child(
      Column::new()
        .child(Outlet::mount(ctx))
        .flex(1.0)
        .width(FILL)
        .padding_left(safe_area.left)
        .padding_right(safe_area.right)
        .padding_bottom(safe_area.bottom),
    )
    .size(FILL, FILL)
    .background(BG)
    .into()
}

fn header(ctx: &mut Ctx, path: &str, safe_area: SafeAreaInsets) -> Element {
  let at_home = !home::SCREENS.iter().any(|screen| screen.path == path);
  let back: Element = if at_home {
    Spacer::new().width(TAP_TARGET).into()
  } else {
    let navigator = ctx.navigator();
    Row::new()
      .id("back")
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .child(text("‹", 28.0, FontWeight::Normal, PRIMARY))
      .size(TAP_TARGET, TAP_TARGET)
      .rounded(8.0)
      .active(|style| style.background("#273449"))
      .on_click(move |_| {
        if let Some(navigator) = &navigator
          && !navigator.back()
        {
          navigator.replace("/");
        }
      })
      .into()
  };

  Row::new()
    .align_items(Alignment::Center)
    .child(back)
    .child(
      Row::new()
        .justify(Justify::Center)
        .child(text(home::title_of(path), 17.0, FontWeight::Bold, TEXT).nowrap())
        .flex(1.0),
    )
    .child(Spacer::new().width(TAP_TARGET))
    .height(HEADER_HEIGHT + safe_area.top)
    .padding_top(safe_area.top)
    .padding_left(4.0 + safe_area.left)
    .padding_right(4.0 + safe_area.right)
    .width(FILL)
    .background(SURFACE)
    .border_inside(1.0, Color::from_hex(BORDER))
    .into()
}

fn main() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_render_engine_factory(|| Box::new(WgpuRenderEngine::new()));
  #[cfg(feature = "mcp")]
  {
    use lurq::mcp::{McpConfig, Scope};
    let handle = tree.enable_mcp(McpConfig::new().app_name("lurq-mobile-demo").scopes([
      Scope::Observe,
      Scope::Interact,
      Scope::Navigate,
    ]));
    eprintln!(
      "MCP listening on http://127.0.0.1:{}/mcp (token {})",
      handle.port(),
      handle.token()
    );
  }
  tree.mount_root::<MobileDemo>(&mut app, ());
  let window = WinitWindow::new(app, tree).with_title("lurq mobile demo");
  // A phone's screen on the desktop. On iOS a window with a size is that size, in the corner of the screen;
  // without one it covers the screen.
  #[cfg(not(target_os = "ios"))]
  let window = window.with_size(393, 852);
  window.run();
}
