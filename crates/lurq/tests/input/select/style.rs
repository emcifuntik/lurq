use std::sync::{
  Arc,
  atomic::{AtomicBool, Ordering},
};

use lurq::{
  app::{App, Tree, events::MouseButton, theme::PaletteColor},
  components::{Column, Rect, Select, SelectOption, Text},
  core::{ElementRef, Signal},
  layout::{
    quad::QuadContent,
    scrollbar::{ScrollBarStyle, ScrollBarVisibility},
  },
  node::{
    BoxShadow, Element, SelectCheckmarkPosition, SelectIcon, SelectPartStyle, SelectStyle, color::Color,
    padding::Padding,
  },
};

use crate::support::{RenderSnapshot, pointer_click, render_pass_with_app};

const SELECT_ID: &str = "select";

fn options() -> Vec<SelectOption<String>> {
  vec![
    SelectOption::new("sm".to_owned(), "Small"),
    SelectOption::new("md".to_owned(), "Medium"),
    SelectOption::new("lg".to_owned(), "Large"),
  ]
}

struct Fixture {
  tree: Tree,
  app: App,
  select: ElementRef,
}

impl Fixture {
  /// A select with the default trigger, which shows the chevron.
  fn new(value: &str, style: SelectStyle) -> Self {
    Self::build(value, options(), style, false)
  }

  /// A select whose trigger does not repeat the selected label, so option
  /// labels are unique in the tree.
  fn with_options(value: &str, options: Vec<SelectOption<String>>, style: SelectStyle) -> Self {
    Self::build(value, options, style, true)
  }

  fn build(value: &str, options: Vec<SelectOption<String>>, style: SelectStyle, plain_trigger: bool) -> Self {
    let select = ElementRef::new();
    let mut tree = Tree::new();
    let mut control = Select::new(Signal::new(value.to_owned()))
      .options(options)
      .width(200.0)
      .height(40.0)
      .style(style)
      .ref_element(select.clone());
    if plain_trigger {
      control = control.trigger(|_| Text::new("Pick"));
    }
    tree.set_root(Element::from(control).id(SELECT_ID));
    let mut app = App::new();
    render_pass_with_app(&mut tree, &mut app);
    Self { tree, app, select }
  }

  fn render(&mut self) -> RenderSnapshot {
    render_pass_with_app(&mut self.tree, &mut self.app)
  }

  fn pointer_open(&mut self) -> RenderSnapshot {
    let (x, y) = self.select.bounds().center();
    pointer_click(&mut self.tree, x, y, MouseButton::Left);
    self.render()
  }

  fn keyboard_open(&mut self) -> RenderSnapshot {
    self
      .tree
      .get_element_by_id_mut(SELECT_ID)
      .expect("select has an id")
      .focus();
    self
      .tree
      .key_down("Enter".to_owned(), "Enter".to_owned(), false, false, false);
    self.render()
  }

  fn bounds_of(&mut self, text: &str) -> lurq::core::ElementRect {
    self
      .tree
      .find_element(|el| el.text_content() == Some(text))
      .unwrap_or_else(|| panic!("{text} is laid out"))
      .bounds()
  }

  fn menu_bounds(&mut self) -> lurq::core::ElementRect {
    self
      .tree
      .find_element(|el| el.tag_name() == "SelectMenu")
      .expect("menu open")
      .bounds()
  }

  fn has_text(&mut self, text: &str) -> bool {
    self.tree.find_element(|el| el.text_content() == Some(text)).is_some()
  }
}

fn glyphs_colored(snapshot: &RenderSnapshot, color: &str) -> usize {
  let expected = Color::from_hex(color).to_linear_f32_array();
  snapshot
    .glyphs
    .iter()
    .filter(|glyph| {
      glyph
        .color
        .iter()
        .zip(expected.iter())
        .all(|(actual, expected)| (actual - expected).abs() < 0.01)
    })
    .count()
}

#[test]
fn highlight_part_composes_with_selected_and_hover() {
  let fill = Color::from_hex("#334155");
  let ring = BoxShadow::new(0.0, 0.0, 0.0, Color::from_hex("#38bdf8"))
    .spread(2.0)
    .inset();
  let style = SelectStyle::new()
    .option_hovered(SelectPartStyle::new().background(fill))
    .option_selected(SelectPartStyle::new())
    .option_selected_hovered(SelectPartStyle::new().background(fill))
    .option_highlighted(SelectPartStyle::new().box_shadow(ring))
    .single_checkmark(true);
  let mut fixture = Fixture::with_options("md", options(), style);
  let snapshot = fixture.keyboard_open();

  let rings: Vec<_> = snapshot
    .rects
    .iter()
    .filter(|rect| rect.shadow.is_some_and(|shadow| shadow.inset) && rect.width >= 150.0)
    .collect();
  assert_eq!(rings.len(), 1, "the highlighted selected option draws its ring");
  assert!(
    !snapshot.rects.iter().any(|rect| rect.color == fill),
    "the keyboard highlight does not borrow the hover fill once option_highlighted is set"
  );
  assert!(fixture.has_text("\u{2713}"), "the selected option keeps its checkmark");

  let medium = fixture.bounds_of("Medium");
  fixture.tree.mouse_move(medium.x + 20.0, medium.y + 5.0);
  let hovered = fixture.render();
  assert!(
    hovered
      .rects
      .iter()
      .any(|rect| rect.color == fill && rect.width >= 150.0),
    "the pointer hover adds its fill to the highlighted row"
  );
  assert!(
    hovered
      .rects
      .iter()
      .any(|rect| rect.shadow.is_some_and(|shadow| shadow.inset)),
    "and the ring stays"
  );
}

#[test]
fn single_select_checkmark_is_opt_in() {
  let mut fixture = Fixture::new("md", SelectStyle::new());
  fixture.pointer_open();
  assert!(!fixture.has_text("\u{2713}"), "hidden by default");

  let mut fixture = Fixture::new("md", SelectStyle::new().single_checkmark(true));
  fixture.pointer_open();
  assert!(fixture.has_text("\u{2713}"), "shown on the selected option");
}

#[test]
fn checkmark_slot_is_reserved_on_every_option() {
  let style = SelectStyle::new().single_checkmark(true).checkmark_size(14.0);
  let mut fixture = Fixture::with_options("md", options(), style);
  fixture.pointer_open();
  let small = fixture.bounds_of("Small");
  let medium = fixture.bounds_of("Medium");
  assert_eq!(
    small.x, medium.x,
    "labels line up whether or not the option is selected"
  );
  let check = fixture.bounds_of("\u{2713}");
  assert!(check.x < medium.x, "leading by default");
}

#[test]
fn trailing_checkmark_sits_after_the_label() {
  let style = SelectStyle::new()
    .single_checkmark(true)
    .checkmark_size(14.0)
    .checkmark_position(SelectCheckmarkPosition::Trailing)
    .checkmark(SelectIcon::text("*"));
  let mut fixture = Fixture::with_options("md", options(), style);
  fixture.pointer_open();
  let menu = fixture.menu_bounds();
  let medium = fixture.bounds_of("Medium");
  let check = fixture.bounds_of("*");
  assert!(check.x > medium.x + medium.width - 1.0, "after the label");
  assert!(check.x + check.width <= menu.x + menu.width, "inside the row");
  let small = fixture.bounds_of("Small");
  assert_eq!(small.x, medium.x);
}

#[test]
fn open_chevron_replaces_the_closed_one_while_open() {
  let style = SelectStyle::new()
    .chevron(SelectIcon::text("v"))
    .chevron_open(SelectIcon::text("^"))
    .chevron_color(Color::from_hex("#ef4444"))
    .trigger_open(SelectPartStyle::new().background(Color::from_hex("#0f172a")));
  let mut fixture = Fixture::new("md", style.clone());
  let closed = fixture.render();
  assert_eq!(glyphs_colored(&closed, "#ef4444"), 1, "only one chevron paints");

  let mut open_fixture = Fixture::new("md", style.chevron_color(Color::from_hex("#22c55e")));
  let open = open_fixture.pointer_open();
  assert_eq!(
    glyphs_colored(&open, "#22c55e"),
    1,
    "only one chevron paints while open"
  );
  let chevron = open_fixture.bounds_of("^");
  assert!(chevron.width > 0.0);
}

#[test]
fn chevron_swap_follows_the_open_state() {
  let open_color = "#22c55e";
  let closed_color = "#ef4444";
  let style = SelectStyle::new()
    .chevron(SelectIcon::element(move |_| {
      Text::new("v").color(Color::from_hex(closed_color)).into()
    }))
    .chevron_open(SelectIcon::element(move |_| {
      Text::new("^").color(Color::from_hex(open_color)).into()
    }));
  let mut fixture = Fixture::new("md", style);
  let closed = fixture.render();
  assert_eq!(glyphs_colored(&closed, closed_color), 1);
  assert_eq!(glyphs_colored(&closed, open_color), 0);
  let open = fixture.pointer_open();
  assert_eq!(glyphs_colored(&open, closed_color), 0);
  assert_eq!(glyphs_colored(&open, open_color), 1);
}

#[test]
fn element_icon_receives_the_configured_color() {
  let received = Arc::new(AtomicBool::new(false));
  let seen = received.clone();
  let style = SelectStyle::new()
    .chevron_color(Color::from_hex("#ef4444"))
    .chevron(SelectIcon::element(move |color| {
      seen.store(color.is_some(), Ordering::Relaxed);
      Rect::new(8.0, 8.0).into()
    }));
  let _fixture = Fixture::new("md", style);
  assert!(received.load(Ordering::Relaxed));
}

#[test]
fn menu_padding_insets_options_and_menu_shadow_paints() {
  let style = SelectStyle::new()
    .menu(
      SelectPartStyle::new()
        .background(Color::from_hex("#111827"))
        .padding(Padding::all(4.0))
        .box_shadow(BoxShadow::new(0.0, 8.0, 24.0, Color::from_hex("#00000066"))),
    )
    .option(
      SelectPartStyle::new()
        .min_height(32.0)
        .padding(Padding::symmetric(8.0, 0.0))
        .rounded(7.0),
    );
  let mut fixture = Fixture::with_options("md", options(), style);
  let snapshot = fixture.pointer_open();
  let menu = fixture.menu_bounds();
  let small = fixture.bounds_of("Small");
  assert_eq!(small.x, menu.x + 4.0 + 8.0, "menu padding plus option padding");
  assert_eq!(menu.height, 3.0 * 32.0 + 8.0, "rows plus vertical menu padding");
  assert!(
    snapshot
      .rects
      .iter()
      .any(|rect| rect.shadow.is_some_and(|shadow| !shadow.inset)),
    "the menu shadow paints"
  );
}

#[test]
fn trigger_part_shadow_paints() {
  let style = SelectStyle::new().trigger_focused(
    SelectPartStyle::new().box_shadow(BoxShadow::new(0.0, 0.0, 0.0, Color::from_hex("#38bdf8")).spread(2.0)),
  );
  let mut fixture = Fixture::new("md", style);
  let closed = fixture.render();
  assert!(!closed.rects.iter().any(|rect| rect.shadow.is_some()));
  let focused = fixture.keyboard_open();
  assert!(focused.rects.iter().any(|rect| rect.shadow.is_some()));
}

#[test]
fn menu_flips_above_the_trigger_without_room_below() {
  let select = ElementRef::new();
  let mut tree = Tree::new();
  tree.resize(400, 300);
  tree.set_root(
    Column::new().child(Rect::new(200.0, 240.0)).child(
      Select::new(Signal::new("md".to_owned()))
        .options(options())
        .width(200.0)
        .height(40.0)
        .ref_element(select.clone()),
    ),
  );
  let mut app = App::new();
  render_pass_with_app(&mut tree, &mut app);
  let trigger = select.bounds();
  pointer_click(&mut tree, trigger.x + 20.0, trigger.y + 20.0, MouseButton::Left);
  render_pass_with_app(&mut tree, &mut app);
  let menu = tree
    .find_element(|el| el.tag_name() == "SelectMenu")
    .expect("menu open")
    .bounds();
  assert!(
    menu.y + menu.height <= trigger.y + 0.5,
    "menu {menu:?} should sit above trigger {trigger:?}"
  );
}

#[test]
fn menu_min_width_expands_past_trigger_and_fits_viewport() {
  let mut default = Fixture::new("sm", SelectStyle::new());
  default.pointer_open();
  assert_eq!(default.menu_bounds().width, default.select.bounds().width);

  let mut wide = Fixture::new("sm", SelectStyle::new().min_menu_width(260.0));
  wide.pointer_open();
  let trigger = wide.select.bounds();
  let menu = wide.menu_bounds();
  assert_eq!(trigger.width, 200.0);
  assert_eq!(menu.width, 260.0);
  assert_eq!(menu.x, trigger.x);

  wide.tree.resize(240, 600);
  wide.render();
  let menu = wide.menu_bounds();
  assert_eq!(menu.width, 240.0);
  assert_eq!(menu.x, 0.0);
}

#[test]
fn option_detail_renders_under_the_label() {
  let options = vec![
    SelectOption::new("sm".to_owned(), "Small"),
    SelectOption::new("xl".to_owned(), "Extra large")
      .detail("Needs a bigger plan")
      .disabled(true),
  ];
  let mut fixture = Fixture::with_options("sm", options, SelectStyle::new());
  fixture.pointer_open();
  let label = fixture.bounds_of("Extra large");
  let detail = fixture.bounds_of("Needs a bigger plan");
  assert!(
    detail.y >= label.y + label.height - 0.5,
    "the detail line is below the label"
  );
  assert_eq!(detail.x, label.x);
}

#[test]
fn menu_scrollbar_insets_clear_the_menu_corners() {
  let thumb = Color::from_hex("#22c55e");
  let options: Vec<SelectOption<String>> = (0..12)
    .map(|index| SelectOption::new(format!("v{index}"), format!("Option {index}")))
    .collect();
  let style = SelectStyle::new()
    .menu(
      SelectPartStyle::new()
        .background(Color::from_hex("#111827"))
        .border_inside(1.0, Color::from_hex("#334155"))
        .rounded(11.0),
    )
    .option(SelectPartStyle::new().min_height(20.0))
    .max_menu_height(120.0)
    .menu_scrollbar(
      ScrollBarStyle {
        width: 4.0,
        thumb_radius: 2.0,
        min_thumb_length: 24.0,
        thumb_color: thumb,
        track_color: Color::new(0, 0, 0, 0),
        visible: ScrollBarVisibility::Auto,
        ..ScrollBarStyle::default()
      }
      .insets(3.0, 11.0),
    );
  let mut fixture = Fixture::with_options("v0", options, style);
  let snapshot = fixture.pointer_open();
  let menu = fixture.menu_bounds();
  assert_eq!(menu.height, 120.0, "the menu is capped and scrolls");

  let bar = snapshot
    .rects
    .iter()
    .find(|rect| rect.color == thumb)
    .expect("the thumb paints");
  assert_eq!(
    bar.x + bar.width,
    menu.x + menu.width - 3.0,
    "3px from the outer right edge"
  );
  assert_eq!(bar.width, 4.0);
  assert_eq!(bar.y, menu.y + 11.0, "11px below the outer top edge");
  let track = menu.height - 22.0;
  let expected = (track * 120.0 / (12.0 * 20.0)).max(24.0);
  assert!(
    (bar.height - expected).abs() < 0.01,
    "thumb {} vs {expected}",
    bar.height
  );
  assert!(
    !snapshot
      .rects
      .iter()
      .any(|rect| rect.color == Color::new(0, 0, 0, 0) && rect.width == 4.0),
    "no track paints"
  );
}

#[test]
fn scrollbar_insets_default_to_padding() {
  let style = ScrollBarStyle::default();
  assert_eq!(style.resolved_edge_inset(), style.padding);
  assert_eq!(style.resolved_end_inset(), style.padding);
  let inset = ScrollBarStyle::default().insets(3.0, 11.0);
  assert_eq!(inset.resolved_edge_inset(), 3.0);
  assert_eq!(inset.resolved_end_inset(), 11.0);
}

/// Fill colours of the rect quads painted over exactly the select's bounds:
/// the trigger's own fill. The trigger content inside it paints no fill.
fn trigger_fills(theme: impl FnOnce(&App), style: SelectStyle) -> Vec<Color> {
  let select = ElementRef::new();
  let mut tree = Tree::new();
  tree.set_root(
    Select::new(Signal::new("md".to_owned()))
      .options(options())
      .width(200.0)
      .height(40.0)
      .style(style)
      .ref_element(select.clone()),
  );
  let mut app = App::new();
  theme(&app);
  tree.pass_headless(&mut app);
  let bounds = select.bounds();
  tree
    .painted_quads()
    .iter()
    .filter(|quad| (quad.x, quad.y, quad.width, quad.height) == (bounds.x, bounds.y, bounds.width, bounds.height))
    .filter_map(|quad| match quad.content {
      QuadContent::Rect { color, .. } if color.a() > 0 => Some(color),
      _ => None,
    })
    .collect()
}

#[test]
fn default_trigger_fill_follows_the_theme_input_surface() {
  let surface = Color::from_hex("#1e293b");
  let fills = trigger_fills(
    |app| app.theme().set_palette_color(PaletteColor::SurfaceInput, surface),
    SelectStyle::new(),
  );
  assert_eq!(fills, [surface]);
}

#[test]
fn trigger_without_a_background_paints_no_fill() {
  // Orchester's recipient selector: an app-styled trigger part with no fill
  // used to paint lurq's fallback white.
  let unstyled = trigger_fills(|_| {}, SelectStyle::unstyled());
  assert_eq!(unstyled, []);
  let replaced = trigger_fills(
    |_| {},
    SelectStyle::new().trigger(SelectPartStyle::new().padding(Padding::symmetric(8.0, 4.0))),
  );
  assert_eq!(replaced, []);
}

#[test]
fn trigger_without_a_background_keeps_its_border() {
  let border = Color::from_hex("#64748b");
  let select = ElementRef::new();
  let mut tree = Tree::new();
  tree.set_root(
    Select::new(Signal::new("md".to_owned()))
      .options(options())
      .width(200.0)
      .height(40.0)
      .style(SelectStyle::unstyled().trigger(SelectPartStyle::new().border_inside(1.0, border)))
      .ref_element(select.clone()),
  );
  tree.pass_headless(&mut App::new());
  let bounds = select.bounds();
  let trigger: Vec<_> = tree
    .painted_quads()
    .into_iter()
    .filter(|quad| (quad.x, quad.y, quad.width, quad.height) == (bounds.x, bounds.y, bounds.width, bounds.height))
    .collect();
  assert_eq!(trigger.len(), 1, "one quad carries the border");
  assert!(matches!(trigger[0].content, QuadContent::Rect { color, .. } if color.a() == 0));
  assert!(trigger[0].border.is_some());
}

#[test]
fn explicit_trigger_background_is_painted_in_every_state() {
  let fill = Color::from_hex("#0f172a");
  let fills = trigger_fills(
    |_| {},
    SelectStyle::unstyled().trigger(SelectPartStyle::new().background(fill)),
  );
  assert_eq!(fills, [fill]);
  let themed = trigger_fills(
    |app| app.theme().set_palette_color(PaletteColor::SurfaceRaised, fill),
    SelectStyle::unstyled().trigger(SelectPartStyle::new().background(PaletteColor::SurfaceRaised)),
  );
  assert_eq!(themed, [fill]);
}

fn mark(id: &str) -> Element {
  Element::from(Rect::new(12.0, 12.0)).id(id)
}

fn bounds_of_id(fixture: &mut Fixture, id: &str) -> Option<lurq::core::ElementRect> {
  fixture
    .tree
    .find_element(|el| el.id() == Some(id))
    .map(|element| element.bounds())
}

#[test]
fn option_leading_and_trailing_elements_sit_around_the_label() {
  let options = vec![
    SelectOption::new("sm".to_owned(), "Small"),
    SelectOption::new("md".to_owned(), "Medium")
      .leading(mark("lead"))
      .trailing(mark("trail")),
  ];
  let mut fixture = Fixture::with_options("sm", options, SelectStyle::new().option_adornment_gap(6.0));
  assert!(
    bounds_of_id(&mut fixture, "lead").is_none(),
    "a closed select builds no menu"
  );
  fixture.pointer_open();
  let label = fixture.bounds_of("Medium");
  let plain = fixture.bounds_of("Small");
  let lead = bounds_of_id(&mut fixture, "lead").expect("leading element is laid out");
  let trail = bounds_of_id(&mut fixture, "trail").expect("trailing element is laid out");
  let menu = fixture.menu_bounds();

  assert_eq!((lead.width, lead.height), (12.0, 12.0));
  assert_eq!(lead.x, plain.x, "the leading element starts where a plain label does");
  assert_eq!(label.x, lead.x + lead.width + 6.0);
  assert_eq!(label.x + label.width + 6.0, trail.x);
  assert!(
    trail.x + trail.width > menu.x + menu.width - 24.0,
    "the trailing element is at the row's end"
  );
  let centre = |rect: lurq::core::ElementRect| rect.y + rect.height / 2.0;
  assert!((centre(lead) - centre(label)).abs() <= 0.5);
  assert!((centre(trail) - centre(label)).abs() <= 0.5);
}

#[test]
fn option_adornments_are_centred_on_the_label_line_beside_a_detail() {
  let options = vec![
    SelectOption::new("sm".to_owned(), "Small"),
    SelectOption::new("xl".to_owned(), "Extra large")
      .detail("Needs a bigger plan")
      .leading(mark("lead"))
      .trailing(mark("trail")),
  ];
  let mut fixture = Fixture::with_options("sm", options, SelectStyle::new());
  fixture.pointer_open();
  let label = fixture.bounds_of("Extra large");
  let detail = fixture.bounds_of("Needs a bigger plan");
  let centre = |rect: lurq::core::ElementRect| rect.y + rect.height / 2.0;
  for id in ["lead", "trail"] {
    let slot = bounds_of_id(&mut fixture, id).expect("the element is laid out");
    assert!(
      (centre(slot) - centre(label)).abs() <= 0.5,
      "{id} is centred on the label, not on the label and detail block"
    );
  }
  assert_eq!(detail.x, label.x, "the detail line stays under the label");
}

#[test]
fn closed_field_shows_the_selected_options_adornments() {
  let options = || {
    vec![
      SelectOption::new("sm".to_owned(), "Small").leading(mark("lead-sm")),
      SelectOption::new("md".to_owned(), "Medium")
        .leading(mark("lead-md"))
        .trailing(mark("trail-md")),
    ]
  };
  let mut fixture = Fixture::build("md", options(), SelectStyle::new(), false);
  let label = fixture.bounds_of("Medium");
  let lead = bounds_of_id(&mut fixture, "lead-md").expect("the field shows the leading element");
  let trail = bounds_of_id(&mut fixture, "trail-md").expect("the field shows the trailing element");
  assert_eq!(label.x, lead.x + lead.width + 8.0);
  assert_eq!(label.x + label.width + 8.0, trail.x);
  assert!(
    bounds_of_id(&mut fixture, "lead-sm").is_none(),
    "only the selected option's element is on the field"
  );

  let mut unselected = Fixture::build("none", options(), SelectStyle::new(), false);
  assert!(bounds_of_id(&mut unselected, "lead-sm").is_none());
  assert!(bounds_of_id(&mut unselected, "lead-md").is_none());
}

#[test]
fn custom_trigger_receives_the_selected_options_adornments() {
  let mut tree = Tree::new();
  tree.set_root(
    Select::new(Signal::new("md".to_owned()))
      .options([
        SelectOption::new("sm".to_owned(), "Small"),
        SelectOption::new("md".to_owned(), "Medium").leading(mark("lead-md")),
      ])
      .trigger(|state| {
        assert!(state.trailing.is_none());
        state.leading.expect("the selected option has a leading element")
      }),
  );
  render_pass_with_app(&mut tree, &mut App::new());
  assert!(tree.find_element(|el| el.id() == Some("lead-md")).is_some());

  let mut tree = Tree::new();
  tree.set_root(
    Select::multiple(Signal::new(vec!["md".to_owned()]))
      .options([SelectOption::new("md".to_owned(), "Medium").leading(mark("lead-md"))])
      .trigger(|state| {
        assert!(state.leading.is_none(), "a multi-select field shows no option element");
        Text::new("Pick")
      }),
  );
  render_pass_with_app(&mut tree, &mut App::new());
}

#[test]
fn option_detail_spans_the_label_between_the_adornments() {
  let options = vec![
    SelectOption::new("sm".to_owned(), "Small"),
    SelectOption::new("xl".to_owned(), "Extra large")
      .detail("Needs a bigger plan")
      .leading(mark("lead"))
      .trailing(mark("trail")),
  ];
  let mut fixture = Fixture::with_options("sm", options, SelectStyle::new());
  fixture.pointer_open();
  let label = fixture.bounds_of("Extra large");
  let detail = fixture.bounds_of("Needs a bigger plan");
  let trail = bounds_of_id(&mut fixture, "trail").expect("trailing element is laid out");
  assert_eq!(detail.width, label.width, "the detail ends where the label does");
  assert_eq!(detail.x + detail.width + 8.0, trail.x);
  assert!(!fixture.has_text(" "), "the alignment adds no text node to the option");
}

#[test]
fn adornment_builder_follows_the_row_and_field_state() {
  use std::sync::Mutex;

  use lurq::node::SelectAdornmentState;

  let seen: Arc<Mutex<Vec<(&'static str, SelectAdornmentState)>>> = Arc::default();
  let record = |name: &'static str| {
    let seen = seen.clone();
    move |state: &SelectAdornmentState| {
      seen.lock().unwrap().push((name, state.clone()));
      Rect::new(12.0, 12.0)
    }
  };
  let selected = Color::from_hex("#f8fafc");
  let options = vec![
    SelectOption::new("sm".to_owned(), "Small").leading_with(record("sm")),
    SelectOption::new("md".to_owned(), "Medium").trailing_with(record("md")),
    SelectOption::new("lg".to_owned(), "Large")
      .leading_with(record("lg"))
      .disabled(true),
  ];
  let style = SelectStyle::new()
    .trigger(SelectPartStyle::new().text_color(PaletteColor::TextMuted))
    .option_selected(SelectPartStyle::new().text_color(selected));
  let mut fixture = Fixture::build("md", options, style, false);
  let latest = |name: &str| {
    let seen = seen.lock().unwrap();
    seen
      .iter()
      .rev()
      .find(|(seen_name, _)| *seen_name == name)
      .map(|(_, state)| state.clone())
  };

  let field = latest("md").expect("the field builds the selected option's element");
  assert!(field.in_field && field.selected && !field.highlighted && !field.disabled);
  assert_eq!(field.text_color, Some(PaletteColor::TextMuted.into()));
  assert!(latest("sm").is_none(), "a closed select builds no other element");

  seen.lock().unwrap().clear();
  fixture.keyboard_open();
  let rows = seen.lock().unwrap().iter().filter(|(_, state)| !state.in_field).count();
  assert!(rows >= 3, "every row builds its element");
  let row = |name: &str| {
    let seen = seen.lock().unwrap();
    seen
      .iter()
      .rev()
      .find(|(seen_name, state)| *seen_name == name && !state.in_field)
      .map(|(_, state)| state.clone())
      .expect("the row builds its element")
  };
  let (small, medium, large) = (row("sm"), row("md"), row("lg"));
  assert!(!small.selected && !small.highlighted && small.text_color.is_none());
  assert!(medium.selected && medium.highlighted);
  assert_eq!(medium.text_color, Some(selected.into()));
  assert!(large.disabled && !large.selected && !large.highlighted);

  seen.lock().unwrap().clear();
  fixture
    .tree
    .key_down("ArrowUp".to_owned(), "ArrowUp".to_owned(), false, false, false);
  fixture.render();
  assert!(row("sm").highlighted, "moving the highlight rebuilds the element");
  assert!(!row("md").highlighted);
}
