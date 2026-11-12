//! The second tab: a stack of its own beside the first, to see that each tab keeps its screens, and what the
//! set of tabs can be told: a badge, a bar taken away.

use lurq::{
  app::ctx::Ctx,
  components::{Column, Link, NavigationItem, ScrollVertical, Tabs},
  layout::text_style::FontWeight,
  node::Element,
  router::Routes,
};

use crate::ui::{BG, FILL, TEXT, button, caption, card, label, text};

pub(crate) fn routes() -> Routes {
  Routes::new().layout("/", crate::shell, |routes| {
    routes.route("/", first).route("/deeper", deeper).fallback(first)
  })
}

fn page(ctx: &mut Ctx, content: Column) -> Element {
  let safe_area = ctx.safe_area();
  ScrollVertical::new(content.width(FILL).padding(16.0).spacing(12.0))
    .size(FILL, FILL)
    .background(BG)
    .padding_left(safe_area.left)
    .padding_right(safe_area.right)
    .padding_bottom(safe_area.bottom)
    .into()
}

fn first(ctx: &mut Ctx) -> Element {
  ctx.navigation_item(NavigationItem::new("More"));
  let tabs = ctx.use_context::<Tabs>();
  let mut content = Column::new()
    .child(caption("A tab of its own: go to the other one and come back, and this one is as it was left.").width(FILL));
  if let Some(tabs) = tabs {
    let (more, fewer, hide) = (tabs.clone(), tabs.clone(), tabs.clone());
    let waiting = tabs
      .list()
      .iter()
      .find(|tab| tab.key == "checks")
      .and_then(|tab| tab.item.badge.clone());
    content = content
      .child(card(label(&match &waiting {
        Some(count) => format!("The first tab's badge says {count}"),
        None => "The first tab has no badge".to_owned(),
      })))
      .child(
        button("One more on the first tab's badge", move || {
          let count = more
            .list()
            .iter()
            .find(|tab| tab.key == "checks")
            .and_then(|tab| tab.item.badge.as_ref()?.parse::<u32>().ok())
            .unwrap_or(0);
          more.set_badge("checks", Some((count + 1).to_string()));
        })
        .width(FILL),
      )
      .child(button("Take the badge away", move || fewer.set_badge("checks", None)).width(FILL))
      .child(button("Hide the bar for two seconds", move || hide_for_a_while(&hide)).width(FILL));
  }
  content = content.child(
    Link::build_empty(ctx, "/deeper")
      .child(card(text(
        "A screen deeper in this tab  ›",
        17.0,
        FontWeight::Bold,
        TEXT,
      )))
      .width(FILL),
  );
  for row in 1..=30 {
    content = content.child(card(label(&format!(
      "Row {row}: scroll down, then tap this tab's place again"
    ))));
  }
  page(ctx, content)
}

fn deeper(ctx: &mut Ctx) -> Element {
  ctx.navigation_item(NavigationItem::new("Deeper"));
  page(
    ctx,
    Column::new().child(
      caption("A tap on this tab's place in the bar goes back to the tab's first screen. So does the way back.")
        .width(FILL),
    ),
  )
}

fn hide_for_a_while(tabs: &Tabs) {
  tabs.set_bar_hidden(true);
  let tabs = tabs.clone();
  std::thread::spawn(move || {
    std::thread::sleep(std::time::Duration::from_secs(2));
    tabs.set_bar_hidden(false);
  });
}
