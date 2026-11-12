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
mod inputs;
mod more;
mod scroll;
mod sheets;
mod touch;
mod ui;
mod virtual_list;

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::Ctx,
    events::{HistoryDirection, HistorySwipeEvent},
    wgpu_render::WgpuRenderEngine,
    window::Orientation,
    winit_shell::WinitWindow,
  },
  components::{
    Column, NavigationBar, NavigationItem, NavigationSearch, Row, SheetHost, StackOutlet, StackOutletProps, TabBar,
    TabItem, TabOutlet, TabOutletProps, Tabs,
  },
  core::Signal,
  layout::{layout_kind::Justify, text_style::FontWeight},
  node::{Element, color::Color},
  router::Routes,
};

use crate::ui::{BG, BORDER, FILL, PRIMARY, TEXT};

struct MobileDemo {
  tabs: Tabs,
}

impl Component for MobileDemo {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let said = app_events();
    ctx.window().on_app_event(move |event| {
      said.update(|said| {
        said.insert(0, format!("{event:?}"));
        said.truncate(6);
      });
    });
    // Two tabs, each a stack of screens with a history of its own. The first entry of a history is the
    // screen at the bottom of its stack, which the others go back to.
    let checks = ctx.router(routes());
    checks.replace("/");
    let more = ctx.router(more::routes());
    more.replace("/");
    let tabs = Tabs::new();
    tabs
      .add(
        "checks",
        TabItem::new("Checks")
          .symbol("checklist")
          .selected_symbol("checklist.checked"),
        checks,
      )
      .add(
        "more",
        TabItem::new("More")
          .symbol("ellipsis.circle")
          .selected_symbol("ellipsis.circle.fill"),
        more,
      );
    Self { tabs }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    // The whole app draws back behind a sheet that takes all the room.
    let tabs = self.tabs.clone();
    SheetHost::mount(ctx, move |ctx| {
      let bar = TabBar::dark()
        // Not all there: what scrolls under the bar shows through it, blurred.
        .background(Color::new(30, 41, 59, 190))
        .border(Color::from_hex(BORDER))
        .tint(Color::from_hex(PRIMARY));
      TabOutlet::mount(ctx, TabOutletProps::new(tabs.clone()).bar(bar))
    })
  }
}

/// What the system said of the app, the latest first; the home screen shows it.
pub(crate) fn app_events() -> Signal<Vec<String>> {
  static SAID: std::sync::OnceLock<Signal<Vec<String>>> = std::sync::OnceLock::new();
  SAID.get_or_init(|| Signal::new(Vec::new())).clone()
}

fn routes() -> Routes {
  Routes::new().layout("/", shell, |routes| {
    routes
      .route("/", |ctx| screen(ctx, "/", home::home))
      .route("/touch", |ctx| {
        screen(ctx, "/touch", |ctx| ctx.mount::<touch::TouchScreen>(()))
      })
      .route("/gestures", |ctx| {
        screen(ctx, "/gestures", |ctx| ctx.mount::<gestures::GesturesScreen>(()))
      })
      .route("/inputs", |ctx| {
        screen(ctx, "/inputs", |ctx| ctx.mount::<inputs::InputsScreen>(()))
      })
      .route("/scroll", |ctx| {
        screen(ctx, "/scroll", |ctx| ctx.mount::<scroll::ScrollScreen>(()))
      })
      .route("/sheets", |ctx| {
        screen(ctx, "/sheets", |ctx| ctx.mount::<sheets::SheetsScreen>(()))
      })
      .route("/virtual-list", |ctx| {
        screen(ctx, "/virtual-list", |ctx| {
          ctx.mount::<virtual_list::VirtualListScreen>(())
        })
      })
      .fallback(|ctx| screen(ctx, "/", home::home))
  })
}

/// The stack of screens under its bar. A finger in from the left edge takes the top screen back with it; from
/// the right edge, or a trackpad's swipe, the history moves as the event says.
pub(crate) fn shell(ctx: &mut Ctx) -> Element {
  let navigator = ctx.navigator();
  let bar = NavigationBar::dark()
    // Not all there: a list that goes on under the bar shows through it, blurred.
    .background(Color::new(30, 41, 59, 190))
    .tint(Color::from_hex(PRIMARY))
    .title(Color::from_hex(TEXT))
    .border(Color::from_hex(BORDER));
  Column::new()
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
    .child(StackOutlet::mount_with(
      ctx,
      StackOutletProps::new().bar(bar).background(Color::from_hex(BG)),
    ))
    .size(FILL, FILL)
    .background(BG)
    .into()
}

/// A screen of the stack: what the bar says over it, and its content inside the safe area. Backgrounds reach
/// the edges of the display; what is read and touched stays clear of the camera housing and the home
/// indicator.
fn screen(ctx: &mut Ctx, path: &str, content: impl FnOnce(&mut Ctx) -> Element) -> Element {
  // The home screen and the inputs have their title large, over content that scrolls it away.
  let large = matches!(path, "/" | "/inputs");
  let item = NavigationItem::new(home::title_of(path)).large_title(large);
  ctx.navigation_item(match path {
    // The home screen is searched; the scroll screen has a prompt over its title and a toolbar under it.
    "/" => item.search(NavigationSearch::new(home::sought()).placeholder("Search the screens")),
    "/scroll" => item.prompt("A prompt over the title").toolbar(
      Row::new()
        .width(FILL)
        .justify(Justify::SpaceBetween)
        .child(ui::text("A toolbar", 17.0, FontWeight::Normal, PRIMARY))
        .child(ui::text("of the screen's", 17.0, FontWeight::Normal, PRIMARY)),
    ),
    _ => item,
  });
  // Under the bar of tabs there is nothing more to keep clear of at the bottom.
  let safe_area = ctx.safe_area();
  Column::new()
    .child(content(ctx))
    .size(FILL, FILL)
    .padding_left(safe_area.left)
    .padding_right(safe_area.right)
    // The home screen's list goes on under the bar of tabs, and shows through it: it pads its own content.
    .padding_bottom(if path == "/" { 0.0 } else { safe_area.bottom })
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
  let window = WinitWindow::new(app, tree)
    .with_title("lurq mobile demo")
    // Upright or on either side; the home screen shows the way the device is turned.
    .with_orientations(Orientation::Portrait | Orientation::LandscapeLeft | Orientation::LandscapeRight);
  // A phone's screen on the desktop. On iOS a window with a size is that size, in the corner of the screen;
  // without one it covers the screen.
  #[cfg(not(target_os = "ios"))]
  let window = window.with_size(393, 852);
  window.run();
}
