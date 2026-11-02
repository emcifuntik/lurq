//! A styled text input with a small toolbar: bold, italic, inline code,
//! strikethrough and links over the selection (or for the next typed text),
//! Ctrl+B / Ctrl+I (Cmd on macOS) while typing, copy as Markdown, and the
//! text and its spans printed as they change. Ctrl+click (Cmd+click) a link
//! to see it reported.
//!
//! cargo run -p lurq --example styled_editor --features winit,wgpu,clipboard

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::Ctx,
    events::{KeyboardEvent, MouseEvent},
    wgpu_render::WgpuRenderEngine,
    winit_shell::WinitWindow,
  },
  components::{
    Button, Column, LinkClick, MarkPresence, Row, SelectionStyle, SpanStyle, StyledChange, StyledSpan, StyledText,
    Text, TextInput, TextInputController, TextMark,
  },
  core::Signal,
  layout::text_style::TextStyle,
  node::Element,
};

struct StyledEditor {
  value: Signal<String>,
  spans: Signal<Vec<StyledSpan>>,
  link_target: Signal<String>,
  /// What the app last heard: the Markdown it would store, the spans, a click.
  report: Signal<String>,
  /// Bumped when the caret or selection may have moved, so the toolbar
  /// shows the marks there.
  toolbar: Signal<SelectionStyle>,
  controller: TextInputController,
}

impl Component for StyledEditor {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let text = "Select some words and press B, I, code or Link. Ctrl+click the docs link.";
    let docs = text.find("docs").unwrap_or(0);
    Self {
      value: ctx.signal(text.to_owned()),
      spans: ctx.signal(vec![
        StyledSpan::new(0..6, SpanStyle::new().bold()),
        StyledSpan::new(
          docs..docs + 4,
          SpanStyle::new().link("https://ivanzaida.github.io/lurq/"),
        ),
      ]),
      link_target: ctx.signal("https://example.com".to_owned()),
      report: ctx.signal(String::new()),
      toolbar: ctx.signal(SelectionStyle::default()),
      controller: TextInputController::new(),
    }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .spacing(12.0)
      .padding(24.0)
      .child(self.toolbar_row())
      .child(self.editor())
      .child(
        Row::new()
          .spacing(8.0)
          .child(Text::new("Link target"))
          .child(TextInput::new(self.link_target.clone()).single_line().width(320.0)),
      )
      .child(Text::new(&self.report.get()).width(560.0))
  }
}

impl StyledEditor {
  fn toolbar_row(&self) -> Row {
    let style = self.toolbar.get();
    let mark_button = |label: &str, mark: TextMark| {
      let pressed = style.mark(mark) == MarkPresence::All;
      self.command(
        &format!("{label}{}", if pressed { " ✓" } else { "" }),
        move |controller| {
          let _ = controller.toggle_mark(mark);
        },
      )
    };
    let target = self.link_target.clone();
    Row::new()
      .spacing(6.0)
      .child(mark_button("B", TextMark::Bold))
      .child(mark_button("I", TextMark::Italic))
      .child(mark_button("code", TextMark::Code))
      .child(mark_button("S", TextMark::Strikethrough))
      .child(self.command("Link", move |controller| {
        let _ = controller.set_link(Some(&target.get_untracked()));
      }))
      .child(self.command("Unlink", |controller| {
        let _ = controller.set_link(None);
      }))
  }

  /// A toolbar button that leaves the keyboard focus in the editor.
  fn command(&self, label: &str, run: impl Fn(&TextInputController) + Send + Sync + 'static) -> Button {
    let controller = self.controller.clone();
    let toolbar = self.toolbar.clone();
    Button::new(label)
      .focusable(false)
      .padding(6.0)
      .on_click(move |_: MouseEvent| {
        run(&controller);
        refresh(&controller, &toolbar);
      })
  }

  fn editor(&self) -> TextInput {
    let report = self.report.clone();
    let clicked = self.report.clone();
    let shortcuts = self.controller.clone();
    let (moved, moved_toolbar) = (self.controller.clone(), self.toolbar.clone());
    let (clicked_controller, clicked_toolbar) = (self.controller.clone(), self.toolbar.clone());
    let changed = (self.controller.clone(), self.toolbar.clone());
    TextInput::styled(
      self.value.clone(),
      TextStyle {
        font_size: 16.0,
        line_height: 1.5,
        ..TextStyle::default()
      },
    )
    .spans(self.spans.clone())
    .controller(self.controller.clone())
    .multiline()
    .rows(4, 12)
    .width(560.0)
    .padding(10.0)
    .copy_format(markdown)
    .on_styled_change(move |change: &StyledChange| {
      report.set(format!(
        "Markdown: {}\n\nEdit: {:?}\n\nSpans: {:#?}",
        markdown(&change.to_styled_text()),
        change.edit(),
        change.spans()
      ));
      refresh(&changed.0, &changed.1);
    })
    .on_link_click(move |link: &LinkClick| clicked.set(format!("Open {} ({:?})", link.target, link.range)))
    .on_key_down(move |event: KeyboardEvent| {
      let command = if cfg!(target_os = "macos") {
        event.meta
      } else {
        event.ctrl
      };
      let mark = match event.key.as_str() {
        "b" | "B" => TextMark::Bold,
        "i" | "I" => TextMark::Italic,
        _ => return,
      };
      if command {
        let _ = shortcuts.toggle_mark(mark);
        event.prevent_default();
      }
    })
    .on_key_up(move |_: KeyboardEvent| refresh(&moved, &moved_toolbar))
    .on_mouse_up(move |_: MouseEvent| refresh(&clicked_controller, &clicked_toolbar))
  }
}

/// Shows the marks at the caret or over the selection in the toolbar.
fn refresh(controller: &TextInputController, toolbar: &Signal<SelectionStyle>) {
  if let Ok(Some(style)) = controller.selection_style()
    && toolbar.get_untracked() != style
  {
    toolbar.set(style);
  }
}

/// Styled text as Markdown, one run at a time: enough for the example; an
/// editor that stores Markdown nests and escapes marks properly.
fn markdown(text: &StyledText) -> String {
  text
    .runs()
    .into_iter()
    .map(|(run, style)| {
      let mut out = run.to_owned();
      if style.code {
        out = format!("`{out}`");
      }
      if style.bold {
        out = format!("**{out}**");
      }
      if style.italic {
        out = format!("_{out}_");
      }
      if style.strikethrough {
        out = format!("~~{out}~~");
      }
      if let Some(target) = style.link {
        out = format!("[{out}]({target})");
      }
      out
    })
    .collect()
}

fn main() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_render_engine_factory(|| Box::new(WgpuRenderEngine::new()));
  tree.mount_root::<StyledEditor>(&mut app, ());
  WinitWindow::new(app, tree)
    .with_title("lurq styled editor")
    .with_size(720, 640)
    .run();
}
