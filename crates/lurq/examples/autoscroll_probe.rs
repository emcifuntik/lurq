//! Middle-button autoscroll (lurq#78), driven through the embedded MCP
//! server: a long virtualized list with autoscroll on, a list with it off,
//! and a list with it on whose rows hold a middle-clickable link and
//! selectable text.
//!
//! cargo run -p lurq --example autoscroll_probe --features winit,wgpu,mcp
use lurq::{
  app::{
    App, Tree, component::Component, ctx::Ctx, events::MouseButton, wgpu_render::WgpuRenderEngine,
    winit_shell::WinitWindow,
  },
  components::{Column, Rect, Row, ScrollVertical, Text, VirtualizedList},
  core::Signal,
  layout::layout_kind::ScrollState,
  mcp::McpConfig,
  node::{Element, color::Color},
};

struct ChatRow;

impl Component for ChatRow {
  type Props = usize;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let index = *ctx.props::<usize>();
    Text::new(&format!("Entry {index}"))
      .id(format!("entry-{index}"))
      .height(24.0)
  }
}

struct Probe {
  chat: ScrollState,
  plain: ScrollState,
  links: ScrollState,
  middle_clicks: Signal<u32>,
  left_clicks: Signal<u32>,
  chat_scrolls: Signal<u32>,
}

impl Component for Probe {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self {
      chat: ScrollState::new(),
      plain: ScrollState::new(),
      links: ScrollState::new(),
      middle_clicks: Signal::new(0),
      left_clicks: Signal::new(0),
      chat_scrolls: Signal::new(0),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let status = Text::new(&format!(
      "c={:.0} p={:.0} l={:.0} m={} L={} e={}",
      self.chat.scroll_y(),
      self.plain.scroll_y(),
      self.links.scroll_y(),
      self.middle_clicks.get(),
      self.left_clicks.get(),
      self.chat_scrolls.get(),
    ))
    .id("status");

    let scrolls = self.chat_scrolls.clone();
    let chat = VirtualizedList::new(ctx, 0..5_000_usize)
      .with_scroll_state(self.chat.clone())
      .middle_button_autoscroll(true)
      .on_scroll(move |_| scrolls.update(|count| *count += 1))
      .size(260.0, 480.0)
      .mount_keyed::<ChatRow, _, _, _>(|item| *item, |item| *item);

    let mut plain_rows = Column::new().spacing(0.0);
    for index in 0..200 {
      plain_rows = plain_rows.child(Text::new(&format!("Plain {index}")).height(24.0));
    }
    let plain_scrolls = self.chat_scrolls.clone();
    let plain = ScrollVertical::new(plain_rows)
      .with_scroll_state(self.plain.clone())
      .on_scroll(move |_| plain_scrolls.update(|count| *count += 1))
      .size(200.0, 480.0)
      .id("plain");

    let mut link_rows = Column::new().spacing(0.0);
    for index in 0..100 {
      let middle = self.middle_clicks.clone();
      let left = self.left_clicks.clone();
      link_rows = link_rows.child(
        Row::new()
          .spacing(8.0)
          .child(
            Rect::new(60.0, 20.0)
              .background(Color::from_hex("#bfdbfe"))
              .id(format!("link-{index}"))
              .on_click(move |_| left.update(|count| *count += 1))
              .on_mouse_click(MouseButton::Middle, move |_| middle.update(|count| *count += 1)),
          )
          .child(
            Text::new(&format!("Selectable line {index}"))
              .selectable(true)
              .id(format!("line-{index}")),
          )
          .height(28.0),
      );
    }
    let links_scrolls = self.chat_scrolls.clone();
    let links = ScrollVertical::new(link_rows)
      .with_scroll_state(self.links.clone())
      .on_scroll(move |_| links_scrolls.update(|count| *count += 1))
      .middle_button_autoscroll(true)
      .size(260.0, 480.0)
      .id("links");

    Column::new().spacing(8.0).child(status).child(
      Row::new()
        .spacing(12.0)
        .child(Column::new().id("chat").child(chat))
        .child(plain)
        .child(links),
    )
  }
}

fn main() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_render_engine_factory(|| Box::new(WgpuRenderEngine::new()));
  tree.mount_root::<Probe>(&mut app, ());
  let mcp = tree.enable_mcp(McpConfig::new().app_name("lurq-autoscroll-probe"));
  println!("MCP on http://127.0.0.1:{}/mcp", mcp.port());
  WinitWindow::new(app, tree)
    .with_size(780, 560)
    .with_title("Autoscroll probe")
    .with_start_without_focus(true)
    .run();
}
