//! The second tab: a stack of its own beside the first, to see that each tab keeps its screens, and what the
//! set of tabs can be told: a badge, a bar taken away.

use lurq::{
  app::ctx::Ctx,
  components::{Column, Link, NavigationItem, NavigationSearch, StackOutlet, TabItem, Tabs},
  core::Signal,
  layout::text_style::FontWeight,
  node::Element,
  router::{RouterHandle, Routes},
};

use crate::ui::{BG, FILL, TEXT, button, caption, card, label, text};

pub(crate) fn routes() -> Routes {
  Routes::new().layout("/", crate::shell, |routes| {
    routes.route("/", first).route("/deeper", deeper).fallback(first)
  })
}

/// The tab to search in: one screen, searched.
pub(crate) fn search_routes() -> Routes {
  Routes::new().layout("/", crate::shell, |routes| {
    routes.route("/", searched).fallback(searched)
  })
}

fn sought() -> Signal<String> {
  static SOUGHT: std::sync::OnceLock<Signal<String>> = std::sync::OnceLock::new();
  SOUGHT.get_or_init(|| Signal::new(String::new())).clone()
}

fn searched(ctx: &mut Ctx) -> Element {
  ctx.navigation_item(
    NavigationItem::new("Search")
      .large_title(true)
      // The title stays while the tab is searched in; the home screen's field takes the bar's place.
      .search(
        NavigationSearch::new(sought())
          .placeholder("Search the rows")
          .hides_bar(false),
      ),
  );
  let sought = sought().get().to_lowercase();
  let mut content = Column::new();
  for row in (1..=40)
    .map(|row| format!("Row {row}"))
    .filter(|row| row.to_lowercase().contains(&sought))
  {
    content = content.child(card(label(&row)));
  }
  page(ctx, content)
}

/// A tab added from the second tab's screen.
pub(crate) fn spare_routes(number: usize) -> Routes {
  Routes::new().layout("/", crate::shell, move |routes| {
    routes.fallback(move |ctx| {
      ctx.navigation_item(NavigationItem::new(format!("Extra {number}")));
      page(
        ctx,
        Column::new().child(
          caption(
            "A tab added while the app runs. With more tabs than the bar has places, the last place lists the rest.",
          )
          .width(FILL),
        ),
      )
    })
  })
}

fn spares() -> &'static std::sync::Mutex<Vec<RouterHandle>> {
  static SPARES: std::sync::Mutex<Vec<RouterHandle>> = std::sync::Mutex::new(Vec::new());
  &SPARES
}

pub(crate) fn set_spares(routers: Vec<RouterHandle>) {
  *spares().lock().unwrap() = routers;
}

fn page(ctx: &mut Ctx, content: Column) -> Element {
  let safe_area = ctx.safe_area();
  // The stack's list goes on under the navigation bar and the bar of tabs, which it shows through, and keeps
  // its content clear of both.
  StackOutlet::large_title_scroll(ctx, content.width(FILL).padding(16.0).spacing(12.0))
    .size(FILL, FILL)
    .background(BG)
    .padding_left(safe_area.left)
    .padding_right(safe_area.right)
    .into()
}

fn first(ctx: &mut Ctx) -> Element {
  ctx.navigation_item(NavigationItem::new("More"));
  let tabs = ctx.use_context::<Tabs>();
  let opener = ctx.window_opener();
  let mut content = Column::new()
    .child(caption("A tab of its own: go to the other one and come back, and this one is as it was left.").width(FILL))
    .child(
      caption("Another window of the app: a tablet shows it beside this one, a phone when a display is plugged in.")
        .width(FILL),
    )
    .child(button("Open another window", move || crate::other_window::open(&opener, false)).width(FILL));
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
      .child(button("Hide the bar for two seconds", move || hide_for_a_while(&hide)).width(FILL))
      .child(
        button("Add a tab", {
          let tabs = tabs.clone();
          move || {
            // The next of the tabs made at the start; the bar has five places, and lists the rest.
            let number = tabs.len() - 2;
            if let Some(router) = spares().lock().unwrap().get(number - 1).cloned() {
              tabs.add(
                format!("extra-{number}"),
                TabItem::new(format!("Extra {number}")).symbol("star"),
                router,
              );
            }
          }
        })
        .width(FILL),
      );
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
  // This screen has the whole window: the bar of tabs goes as it comes in, and comes back with the way back.
  ctx.navigation_item(NavigationItem::new("Deeper").hides_tab_bar(true));
  page(
    ctx,
    Column::new().child(
      caption("This screen hides the bar of tabs. Go back, slowly with a finger from the left edge, and the bar comes back with it.")
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
