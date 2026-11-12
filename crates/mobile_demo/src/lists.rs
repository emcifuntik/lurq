//! What a phone's lists do under a finger: pulled down past the top a list loads anew, and a row swiped
//! aside shows what can be done with it.

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{
    Column, PullToRefresh, PullToRefreshProps, PullToRefreshStyle, Row, StackOutlet, SwipeAction, SwipeRow,
    SwipeRowProps,
  },
  core::Signal,
  layout::{Alignment, text_style::FontWeight},
  node::{Element, color::Color},
};

use crate::ui::{BG, BORDER, FILL, PRIMARY, SURFACE, TEXT, caption, text};

pub(crate) struct ListsScreen {
  rows: Signal<Vec<String>>,
  refreshing: Signal<bool>,
  /// How many times the list was loaded anew, and what was last done with a row.
  loads: Signal<u32>,
  did: Signal<String>,
}

impl Component for ListsScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      rows: ctx.signal((1..=12).map(|row| format!("Row {row}")).collect()),
      refreshing: ctx.signal(false),
      loads: ctx.signal(0),
      did: ctx.signal(String::new()),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let rows = self.rows.get();
    let did = self.did.get();
    let mut content = Column::new().width(FILL).padding_vertical(16.0).spacing(1.0).child(
      Row::new().padding_horizontal(16.0).padding_bottom(12.0).child(
        caption(&format!(
          "Pull the list down to load it anew ({} so far). Swipe a row left for Delete and Flag, right for Pin; all the way left deletes. {}",
          self.loads.get(),
          if did.is_empty() { String::new() } else { format!("Last: {did}.") }
        ))
        .width(FILL),
      ),
    );
    for name in &rows {
      let (delete, flag, pin) = (name.clone(), name.clone(), name.clone());
      let (all, did_delete, did_flag, did_pin) =
        (self.rows.clone(), self.did.clone(), self.did.clone(), self.did.clone());
      content = content.child(SwipeRow::mount_keyed(
        ctx,
        name,
        SwipeRowProps::new()
          .trailing([
            SwipeAction::destructive("Delete", move || {
              all.update(|rows| rows.retain(|row| row != &delete));
              did_delete.set(format!("deleted {delete}"));
            }),
            SwipeAction::new("Flag", move || did_flag.set(format!("flagged {flag}")))
              .background(Color::from_hex("#FF9500")),
          ])
          .leading([
            SwipeAction::new("Pin", move || did_pin.set(format!("pinned {pin}"))).background(Color::from_hex(PRIMARY))
          ]),
        Row::new()
          .width(FILL)
          .height(56.0)
          .align_items(Alignment::Center)
          .padding_horizontal(16.0)
          .background(SURFACE)
          .border_inside(0.5, Color::from_hex(BORDER))
          .child(text(name, 17.0, FontWeight::Normal, TEXT)),
      ));
    }

    // The stack's own list, which goes on under the bars; the spinner stands under the navigation bar.
    let top = StackOutlet::bar_inset(ctx);
    let scroll = StackOutlet::screen_scroll(ctx).unwrap_or_else(lurq::layout::layout_kind::ScrollState::new);
    let list = StackOutlet::large_title_scroll(ctx, content)
      .size(FILL, FILL)
      .background(BG);
    let (refreshing, loads, all) = (self.refreshing.clone(), self.loads.clone(), self.rows.clone());
    PullToRefresh::mount(
      ctx,
      PullToRefreshProps::new(self.refreshing.clone(), scroll)
        .top(top)
        .style(PullToRefreshStyle::dark())
        .on_refresh(move || {
          // What an app would load: here, a wait and one more row.
          let (refreshing, loads, all) = (refreshing.clone(), loads.clone(), all.clone());
          std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            loads.update(|loads| *loads += 1);
            let number = loads.get_untracked();
            all.update(|rows| rows.insert(0, format!("Loaded {number}")));
            refreshing.set(false);
          });
        }),
      list,
    )
  }
}
