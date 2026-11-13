use std::sync::Arc;

use crate::{
  app::ctx::OutsidePress,
  core::{ElementRef, Signal, SignalValue},
  layout::{Alignment, StackAlignment, layout_kind::Justify},
  node::{
    Element, Node, SelectAdornmentState, SelectIcon, SelectStyle, SyntheticNodeRole,
    dimension::Dimension,
    node_kind::{SelectChangeCallback, TextOverflow},
    select_icon::SelectAdornment,
  },
};

enum Binding<T>
where
  T: SignalValue,
  Vec<T>: SignalValue,
{
  Single(Signal<T>),
  Multiple(Signal<Vec<T>>),
}

/// One option of a [`Select`]: a value, its label, an optional detail line
/// drawn under the label (for example why the option is disabled), optional
/// elements before and after the label (a brand mark, a status dot, a badge),
/// and whether it is disabled. Disabled options cannot be chosen by pointer
/// or keyboard, never take the hover or keyboard highlight, and are skipped
/// by arrow keys, Home/End and type-ahead.
///
/// `(value, label)` tuples convert into enabled options without a detail.
#[derive(Clone)]
pub struct SelectOption<T> {
  value: T,
  label: Arc<str>,
  detail: Option<Arc<str>>,
  leading: Option<SelectAdornment>,
  trailing: Option<SelectAdornment>,
  disabled: bool,
}

impl<T> SelectOption<T> {
  pub fn new(value: T, label: impl Into<Arc<str>>) -> Self {
    Self {
      value,
      label: label.into(),
      detail: None,
      leading: None,
      trailing: None,
      disabled: false,
    }
  }

  /// A second, smaller line under the label, styled by
  /// `SelectStyle::option_detail`.
  pub fn detail(mut self, detail: impl Into<Arc<str>>) -> Self {
    self.detail = Some(detail.into());
    self
  }

  /// An element drawn before the label, such as an icon or a brand mark. It
  /// keeps its own size, sits `SelectStyle::option_adornment_gap` from the
  /// label and is centred on the label's line. The closed field of a
  /// single-select shows it for the selected option.
  ///
  /// The element is decorative: the option's name for type-ahead and
  /// automation stays its label. Built menus are plain nodes, so pass an
  /// icon, SVG, text or layout element, not a stateful component.
  ///
  /// The same element is drawn in every state of the row; use
  /// [`leading_with`](Self::leading_with) for one that follows the state.
  pub fn leading(mut self, element: impl Into<Element>) -> Self {
    self.leading = Some(fixed_adornment(element.into()));
    self
  }

  /// A leading element built for the state it is drawn in: whether the
  /// option is selected, highlighted or disabled, whether it is on the closed
  /// field, and the text colour of the row or field. Called each time the
  /// row or field is built, so a mark can take the label's colour:
  ///
  /// ```ignore
  /// SelectOption::new(value, "Claude Code").leading_with(|state| mark(state.text_color.clone()))
  /// ```
  pub fn leading_with<E>(mut self, build: impl Fn(&SelectAdornmentState) -> E + Send + Sync + 'static) -> Self
  where
    E: Into<Element>,
  {
    self.leading = Some(Arc::new(move |state| build(state).into()));
    self
  }

  /// An element drawn after the label, at the row's trailing edge; otherwise
  /// like [`leading`](Self::leading).
  pub fn trailing(mut self, element: impl Into<Element>) -> Self {
    self.trailing = Some(fixed_adornment(element.into()));
    self
  }

  /// A trailing element built for the state it is drawn in; see
  /// [`leading_with`](Self::leading_with).
  pub fn trailing_with<E>(mut self, build: impl Fn(&SelectAdornmentState) -> E + Send + Sync + 'static) -> Self
  where
    E: Into<Element>,
  {
    self.trailing = Some(Arc::new(move |state| build(state).into()));
    self
  }

  pub fn disabled(mut self, disabled: bool) -> Self {
    self.disabled = disabled;
    self
  }
}

/// An adornment that draws a copy of `element` in every state.
fn fixed_adornment(element: Element) -> SelectAdornment {
  // An element is `Send` but not `Sync`; the lock makes the builder both.
  let element = std::sync::Mutex::new(element);
  Arc::new(move |_| element.lock().unwrap().clone())
}

impl<T, L> From<(T, L)> for SelectOption<T>
where
  L: Into<Arc<str>>,
{
  fn from((value, label): (T, L)) -> Self {
    Self::new(value, label)
  }
}

/// A native, generic dropdown select. Single-select binds a `Signal<T>`;
/// multi-select binds a `Signal<Vec<T>>`. Options pair a value with a label;
/// the selected value(s) are derived by comparing the bound signal against the
/// option values each render, so the control stays in sync reactively.
pub struct Select<T>
where
  T: SignalValue,
  Vec<T>: SignalValue,
{
  node: Box<Node>,
  binding: Binding<T>,
  options: Vec<SelectOption<T>>,
  placeholder: Option<Arc<str>>,
  style: SelectStyle,
  trigger: Option<Arc<dyn Fn(SelectTriggerState) -> Element + Send + Sync>>,
  outside_press: OutsidePress,
}

#[derive(Clone)]
pub struct SelectTriggerState {
  /// The selected option's leading element; `None` in a multi-select.
  pub leading: Option<Element>,
  /// The selected option's trailing element; `None` in a multi-select.
  pub trailing: Option<Element>,
  pub label: Option<Arc<str>>,
  pub placeholder: Option<Arc<str>>,
  pub selected_labels: Vec<Arc<str>>,
  pub selected_count: usize,
  pub multiple: bool,
}

impl<T> Select<T>
where
  T: Clone + PartialEq + Send + Sync + 'static + SignalValue,
  Vec<T>: SignalValue,
{
  pub fn new(value: Signal<T>) -> Self {
    Self {
      node: Box::new(Node::select()),
      binding: Binding::Single(value),
      options: Vec::new(),
      placeholder: None,
      style: SelectStyle::new(),
      trigger: None,
      outside_press: OutsidePress::default(),
    }
  }

  pub fn multiple(value: Signal<Vec<T>>) -> Self {
    Self {
      node: Box::new(Node::select()),
      binding: Binding::Multiple(value),
      options: Vec::new(),
      placeholder: None,
      style: SelectStyle::new(),
      trigger: None,
      outside_press: OutsidePress::default(),
    }
  }

  /// The options, as `(value, label)` tuples or [`SelectOption`]s.
  pub fn options(mut self, options: impl IntoIterator<Item = impl Into<SelectOption<T>>>) -> Self {
    self.options = options.into_iter().map(Into::into).collect();
    self
  }

  pub fn placeholder(mut self, placeholder: impl Into<Arc<str>>) -> Self {
    self.placeholder = Some(placeholder.into());
    self
  }

  pub fn style(mut self, style: SelectStyle) -> Self {
    self.style = style;
    self
  }

  pub fn style_with(mut self, f: impl FnOnce(SelectStyle) -> SelectStyle) -> Self {
    self.style = f(SelectStyle::new());
    self
  }

  pub fn trigger<R>(mut self, f: impl Fn(SelectTriggerState) -> R + Send + Sync + 'static) -> Self
  where
    R: Into<Element>,
  {
    self.trigger = Some(Arc::new(move |state| f(state).into()));
    self
  }

  pub fn width(mut self, width: impl Into<crate::node::dimension::Dimension>) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::width(node, width));
    self
  }

  fn update_node(&mut self, f: impl FnOnce(&mut Node)) {
    f(&mut self.node);
  }

  pub fn height(mut self, height: impl Into<crate::node::dimension::Dimension>) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::height(node, height));
    self
  }

  pub fn tab_index(mut self, tab_index: i32) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::tab_index(node, tab_index));
    self
  }

  /// `false` keeps the select from taking focus, by click, Tab or request.
  pub fn focusable(mut self, focusable: bool) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::focusable(node, focusable));
    self
  }

  /// Whether the press outside the open menu that closes it also reaches the
  /// element under the pointer. Defaults to [`OutsidePress::Consume`]: the
  /// press only closes the menu, and the select keeps focus. A press on
  /// another dropdown's trigger, such as another select, is delivered either
  /// way and opens that dropdown.
  pub fn outside_press(mut self, outside_press: OutsidePress) -> Self {
    self.outside_press = outside_press;
    self
  }

  pub fn ref_element(mut self, element_ref: impl Into<ElementRef>) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::ref_element(node, element_ref));
    self
  }

  #[cfg(feature = "form")]
  pub fn name(mut self, name: impl Into<Arc<str>>) -> Self {
    self.update_node(|node| crate::node::NodeUpdate::name(node, name));
    self
  }

  fn finalize(self) -> Node {
    let labels: Vec<Arc<str>> = self.options.iter().map(|option| option.label.clone()).collect();
    let details: Vec<Option<Arc<str>>> = self.options.iter().map(|option| option.detail.clone()).collect();
    let disabled: Vec<bool> = self.options.iter().map(|option| option.disabled).collect();
    let mut leading = Vec::with_capacity(self.options.len());
    let mut trailing = Vec::with_capacity(self.options.len());
    let mut values = Vec::with_capacity(self.options.len());
    for option in self.options {
      leading.push(option.leading);
      trailing.push(option.trailing);
      values.push(option.value);
    }

    let (selected, multiple, on_change, binding) = match self.binding {
      Binding::Single(signal) => {
        let binding = signal.id();
        let current = signal.get();
        let selected: Vec<usize> = values.iter().position(|value| *value == current).into_iter().collect();
        let option_values = values.clone();
        let on_change: SelectChangeCallback = Arc::new(move |index| {
          if let Some(value) = option_values.get(index) {
            signal.set(value.clone());
          }
        });
        (selected, false, on_change, binding)
      }
      Binding::Multiple(signal) => {
        let binding = signal.id();
        let current = signal.get();
        let selected: Vec<usize> = values
          .iter()
          .enumerate()
          .filter(|(_, value)| current.contains(value))
          .map(|(index, _)| index)
          .collect();
        let option_values = values.clone();
        let on_change: SelectChangeCallback = Arc::new(move |index| {
          if let Some(value) = option_values.get(index) {
            signal.update(|current| {
              if let Some(position) = current.iter().position(|existing| existing == value) {
                current.remove(position);
              } else {
                current.push(value.clone());
              }
            });
          }
        });
        (selected, true, on_change, binding)
      }
    };
    let selected_labels: Vec<Arc<str>> = selected
      .iter()
      .filter_map(|index| labels.get(*index).cloned())
      .collect();
    let label = match selected_labels.as_slice() {
      [] => self.placeholder.clone(),
      [label] => Some(label.clone()),
      many => Some(Arc::from(format!("{} selected", many.len()))),
    };
    let single = match (multiple, selected.as_slice()) {
      (false, [index]) => Some(*index),
      _ => None,
    };
    let field = SelectAdornmentState {
      selected: true,
      highlighted: false,
      disabled: single.is_some_and(|index| disabled[index]),
      in_field: true,
      text_color: self.style.resolved_trigger(false, false, false).text_color,
    };
    let adornment = |slots: &[Option<SelectAdornment>]| {
      let build = slots.get(single?)?.as_ref()?;
      Some(build(&field))
    };
    let trigger_state = SelectTriggerState {
      leading: adornment(&leading),
      trailing: adornment(&trailing),
      label,
      placeholder: self.placeholder.clone(),
      selected_count: selected.len(),
      selected_labels,
      multiple,
    };
    let trigger = self
      .trigger
      .map(|render| render(trigger_state.clone()).into_node())
      .unwrap_or_else(|| default_trigger(trigger_state, &self.style));

    (*self.node)
      .with_tag_name(Arc::from("Select"))
      .with_children([trigger])
      .select_labels(labels)
      .select_options_meta(details, disabled)
      .select_option_adornments(leading, trailing)
      .select_selected(selected)
      .select_multiple(multiple)
      .select_placeholder(self.placeholder)
      .select_style(self.style)
      .select_on_change(on_change)
      .select_binding(binding)
      .select_outside_press(self.outside_press)
  }
}

fn default_trigger(state: SelectTriggerState, style: &SelectStyle) -> Node {
  let trigger = style.resolved_trigger(false, false, false);
  let is_placeholder = state.selected_count == 0;
  let text = state.label.unwrap_or_default();
  let text_node = match (is_placeholder, style.placeholder_text.as_ref()) {
    (true, Some(text_style)) => Node::text_styled(&text, text_style.clone()),
    _ => trigger.text_node(&text),
  }
  .text_wrap(false)
  .text_overflow(TextOverflow::Elipsis)
  .min_width(0.0)
  .flex(1.0);

  let chevron = match &style.chevron_open {
    Some(open_icon) => Node::stack(
      StackAlignment::Center,
      vec![
        with_chevron_role(chevron_node(&style.chevron, &trigger, style), false),
        with_chevron_role(chevron_node(open_icon, &trigger, style), true),
      ],
    ),
    None => chevron_node(&style.chevron, &trigger, style),
  };

  let content = if state.leading.is_none() && state.trailing.is_none() {
    text_node
  } else {
    let slot = |element: Element| element.into_node().flex_shrink(0.0);
    let children = state
      .leading
      .map(slot)
      .into_iter()
      .chain([text_node])
      .chain(state.trailing.map(slot))
      .collect();
    Node::row(style.option_adornment_gap, Alignment::Center, children)
      .min_width(0.0)
      .flex(1.0)
  };

  Node::row(8.0, Alignment::Center, vec![content, chevron])
    .justify(Justify::Start)
    .width(Dimension::Pct(100.0))
}

/// Tags a chevron for one open state; painting skips it in the other.
fn with_chevron_role(mut node: Node, open: bool) -> Node {
  node.set_synthetic_role(SyntheticNodeRole::SelectChevron { open });
  node
}

fn chevron_node(icon: &SelectIcon, trigger: &crate::node::SelectPartStyle, style: &SelectStyle) -> Node {
  let node = icon.build(
    trigger.text_style(),
    Some(style.chevron_size),
    style.chevron_color.as_ref().or(trigger.text_color.as_ref()),
  );
  if icon.is_plain_text() {
    node.width(Dimension::Px(style.chevron_size + 4.0))
  } else {
    node
  }
}

impl<T> From<Select<T>> for Element
where
  T: Clone + PartialEq + Send + Sync + 'static + SignalValue,
  Vec<T>: SignalValue,
{
  fn from(select: Select<T>) -> Self {
    Element::from_node(select.finalize())
  }
}
