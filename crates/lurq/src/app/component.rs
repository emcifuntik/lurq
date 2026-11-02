use std::fmt::{Debug, Display};

use crate::{
  app::ctx::Ctx,
  core::{Signal, SignalValue},
  node::Element,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentInfo {
  name: String,
  type_name: String,
  formatted_value: Option<String>,
  children: Vec<ComponentInfo>,
}

impl ComponentInfo {
  pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
    Self {
      name: name.into(),
      type_name: type_name.into(),
      formatted_value: None,
      children: Vec::new(),
    }
  }

  pub fn with_value(name: impl Into<String>, type_name: impl Into<String>, formatted_value: impl Into<String>) -> Self {
    Self {
      name: name.into(),
      type_name: type_name.into(),
      formatted_value: Some(formatted_value.into()),
      children: Vec::new(),
    }
  }

  pub fn with_children(name: impl Into<String>, type_name: impl Into<String>, children: Vec<ComponentInfo>) -> Self {
    Self {
      name: name.into(),
      type_name: type_name.into(),
      formatted_value: None,
      children,
    }
  }

  pub fn name(&self) -> &str {
    &self.name
  }

  pub fn value(&self) -> &str {
    &self.type_name
  }

  pub fn type_name(&self) -> &str {
    &self.type_name
  }

  pub fn formatted_value(&self) -> Option<&str> {
    self.formatted_value.as_deref()
  }

  pub fn children(&self) -> &[ComponentInfo] {
    &self.children
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn estimated_memory_bytes(&self) -> usize {
    self.name.capacity()
      + self.type_name.capacity()
      + self.formatted_value.as_ref().map(|value| value.capacity()).unwrap_or(0)
      + self.children.capacity() * std::mem::size_of::<ComponentInfo>()
      + self
        .children
        .iter()
        .map(ComponentInfo::estimated_memory_bytes)
        .sum::<usize>()
  }
}

pub struct DevtoolsFormatter<'a> {
  buffer: &'a mut Vec<ComponentInfo>,
}

impl<'a> DevtoolsFormatter<'a> {
  pub fn new(buffer: &'a mut Vec<ComponentInfo>) -> Self {
    Self { buffer }
  }

  pub fn buffer_mut(&mut self) -> &mut Vec<ComponentInfo> {
    self.buffer
  }

  pub fn debug_struct<'b>(&'b mut self, type_name: impl Into<String>) -> DevtoolsStruct<'b, 'a> {
    DevtoolsStruct {
      formatter: self,
      _type_name: type_name.into(),
    }
  }

  pub fn debug_tuple<'b>(&'b mut self, type_name: impl Into<String>) -> DevtoolsTuple<'b, 'a> {
    DevtoolsTuple {
      formatter: self,
      _type_name: type_name.into(),
      index: 0,
    }
  }

  pub fn debug_list<'b>(&'b mut self) -> DevtoolsList<'b, 'a> {
    DevtoolsList {
      formatter: self,
      index: 0,
    }
  }

  pub fn debug_set<'b>(&'b mut self) -> DevtoolsSet<'b, 'a> {
    DevtoolsSet {
      formatter: self,
      index: 0,
    }
  }

  pub fn debug_map<'b>(&'b mut self) -> DevtoolsMap<'b, 'a> {
    DevtoolsMap { formatter: self }
  }

  pub fn value_debug<T: Debug + ?Sized>(&mut self, value: &T) {
    self.value(std::any::type_name::<T>(), format!("{value:?}"));
  }

  pub fn value_display<T: Display + ?Sized>(&mut self, value: &T) {
    self.value(std::any::type_name::<T>(), value.to_string());
  }

  pub fn value(&mut self, type_name: impl Into<String>, formatted_value: impl Into<String>) {
    self
      .buffer
      .push(ComponentInfo::with_value("value", type_name, formatted_value));
  }

  pub fn field<T: DevtoolsInspectable + ?Sized>(&mut self, name: impl Into<String>, value: &T) {
    self.field_as(name, std::any::type_name::<T>(), value);
  }

  pub fn field_as<T: DevtoolsInspectable + ?Sized>(
    &mut self,
    name: impl Into<String>,
    type_name: impl Into<String>,
    value: &T,
  ) {
    let mut children = Vec::new();
    value.write_info(&mut children);
    self.field_children_or_value(name, type_name, children);
  }

  pub fn field_debug<T: Debug + ?Sized>(&mut self, name: impl Into<String>, value: &T) {
    self.field_value(name, std::any::type_name::<T>(), format!("{value:?}"));
  }

  pub fn field_display<T: Display + ?Sized>(&mut self, name: impl Into<String>, value: &T) {
    self.field_value(name, std::any::type_name::<T>(), value.to_string());
  }

  pub fn field_value(
    &mut self,
    name: impl Into<String>,
    type_name: impl Into<String>,
    formatted_value: impl Into<String>,
  ) {
    self
      .buffer
      .push(ComponentInfo::with_value(name, type_name, formatted_value));
  }

  pub fn field_with(
    &mut self,
    name: impl Into<String>,
    type_name: impl Into<String>,
    inspect: impl FnOnce(&mut DevtoolsFormatter<'_>),
  ) {
    let mut children = Vec::new();
    {
      let mut formatter = DevtoolsFormatter::new(&mut children);
      inspect(&mut formatter);
    }
    self
      .buffer
      .push(ComponentInfo::with_children(name, type_name, children));
  }

  fn field_children_or_value(
    &mut self,
    name: impl Into<String>,
    type_name: impl Into<String>,
    children: Vec<ComponentInfo>,
  ) {
    if let Some(value) = collapsed_debug_value(&children) {
      self.buffer.push(ComponentInfo::with_value(name, type_name, value));
    } else {
      self
        .buffer
        .push(ComponentInfo::with_children(name, type_name, children));
    }
  }
}

pub struct DevtoolsStruct<'b, 'a> {
  formatter: &'b mut DevtoolsFormatter<'a>,
  _type_name: String,
}

impl DevtoolsStruct<'_, '_> {
  pub fn field<T: DevtoolsInspectable + ?Sized>(&mut self, name: impl Into<String>, value: &T) -> &mut Self {
    self.formatter.field(name, value);
    self
  }

  pub fn field_as<T: DevtoolsInspectable + ?Sized>(
    &mut self,
    name: impl Into<String>,
    type_name: impl Into<String>,
    value: &T,
  ) -> &mut Self {
    self.formatter.field_as(name, type_name, value);
    self
  }

  pub fn field_debug<T: Debug + ?Sized>(&mut self, name: impl Into<String>, value: &T) -> &mut Self {
    self.formatter.field_debug(name, value);
    self
  }

  pub fn field_display<T: Display + ?Sized>(&mut self, name: impl Into<String>, value: &T) -> &mut Self {
    self.formatter.field_display(name, value);
    self
  }

  pub fn field_value(
    &mut self,
    name: impl Into<String>,
    type_name: impl Into<String>,
    formatted_value: impl Into<String>,
  ) -> &mut Self {
    self.formatter.field_value(name, type_name, formatted_value);
    self
  }

  pub fn finish(&mut self) {}
}

pub struct DevtoolsTuple<'b, 'a> {
  formatter: &'b mut DevtoolsFormatter<'a>,
  _type_name: String,
  index: usize,
}

impl DevtoolsTuple<'_, '_> {
  pub fn field<T: DevtoolsInspectable + ?Sized>(&mut self, value: &T) -> &mut Self {
    let index = self.next_index();
    self.formatter.field(index, value);
    self
  }

  pub fn field_as<T: DevtoolsInspectable + ?Sized>(&mut self, type_name: impl Into<String>, value: &T) -> &mut Self {
    let index = self.next_index();
    self.formatter.field_as(index, type_name, value);
    self
  }

  pub fn field_debug<T: Debug + ?Sized>(&mut self, value: &T) -> &mut Self {
    let index = self.next_index();
    self.formatter.field_debug(index, value);
    self
  }

  pub fn field_value(&mut self, type_name: impl Into<String>, formatted_value: impl Into<String>) -> &mut Self {
    let index = self.next_index();
    self.formatter.field_value(index, type_name, formatted_value);
    self
  }

  pub fn finish(&mut self) {}

  fn next_index(&mut self) -> String {
    let index = self.index;
    self.index += 1;
    index.to_string()
  }
}

pub struct DevtoolsList<'b, 'a> {
  formatter: &'b mut DevtoolsFormatter<'a>,
  index: usize,
}

impl DevtoolsList<'_, '_> {
  pub fn entry<T: DevtoolsInspectable + ?Sized>(&mut self, value: &T) -> &mut Self {
    let index = self.next_index();
    self.formatter.field(index, value);
    self
  }

  pub fn entry_debug<T: Debug + ?Sized>(&mut self, value: &T) -> &mut Self {
    let index = self.next_index();
    self.formatter.field_debug(index, value);
    self
  }

  pub fn finish(&mut self) {}

  fn next_index(&mut self) -> String {
    let index = self.index;
    self.index += 1;
    index.to_string()
  }
}

pub struct DevtoolsSet<'b, 'a> {
  formatter: &'b mut DevtoolsFormatter<'a>,
  index: usize,
}

impl DevtoolsSet<'_, '_> {
  pub fn entry<T: DevtoolsInspectable + ?Sized>(&mut self, value: &T) -> &mut Self {
    let index = self.next_index();
    self.formatter.field(index, value);
    self
  }

  pub fn entry_debug<T: Debug + ?Sized>(&mut self, value: &T) -> &mut Self {
    let index = self.next_index();
    self.formatter.field_debug(index, value);
    self
  }

  pub fn finish(&mut self) {}

  fn next_index(&mut self) -> String {
    let index = self.index;
    self.index += 1;
    index.to_string()
  }
}

pub struct DevtoolsMap<'b, 'a> {
  formatter: &'b mut DevtoolsFormatter<'a>,
}

impl DevtoolsMap<'_, '_> {
  pub fn entry<K: Debug + ?Sized, V: DevtoolsInspectable + ?Sized>(&mut self, key: &K, value: &V) -> &mut Self {
    self.formatter.field(format!("{key:?}"), value);
    self
  }

  pub fn entry_debug<K: Debug + ?Sized, V: Debug + ?Sized>(&mut self, key: &K, value: &V) -> &mut Self {
    self.formatter.field_debug(format!("{key:?}"), value);
    self
  }

  pub fn finish(&mut self) {}
}

fn collapsed_debug_value(fields: &[ComponentInfo]) -> Option<String> {
  if fields.len() != 1 {
    return None;
  }
  let info = &fields[0];
  if matches!(info.name(), "value" | "variant") {
    info.formatted_value().map(ToOwned::to_owned)
  } else {
    None
  }
}

pub trait DevtoolsInspectable {
  fn inspect(&self, _formatter: &mut DevtoolsFormatter<'_>) {}

  fn write_info(&self, buffer: &mut Vec<ComponentInfo>) {
    let mut formatter = DevtoolsFormatter::new(buffer);
    self.inspect(&mut formatter);
  }
}

impl DevtoolsInspectable for () {
  fn inspect(&self, _formatter: &mut DevtoolsFormatter<'_>) {}
}

impl DevtoolsInspectable for &'static str {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.value_debug(self);
  }
}

impl DevtoolsInspectable for std::sync::Arc<str> {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.value_debug(self);
  }
}

macro_rules! impl_scalar_devtools_inspectable {
  ($($ty:ty),* $(,)?) => {
    $(
      impl DevtoolsInspectable for $ty {
        fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
          formatter.value_debug(self);
        }
      }
    )*
  };
}

impl_scalar_devtools_inspectable!(
  bool, char, i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64, String
);

impl<T: DevtoolsInspectable> DevtoolsInspectable for Option<T> {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    match self {
      Some(value) => {
        formatter.field_with("Some", std::any::type_name::<Option<T>>(), |formatter| {
          value.write_info(formatter.buffer_mut());
        });
      }
      None => formatter.value(std::any::type_name::<Option<T>>(), "None"),
    }
  }
}

impl<T: DevtoolsInspectable> DevtoolsInspectable for Vec<T> {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.value(std::any::type_name::<Vec<T>>(), format!("len={}", self.len()));
  }
}

impl<T, S> DevtoolsInspectable for std::collections::HashSet<T, S> {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.value(
      std::any::type_name::<std::collections::HashSet<T, S>>(),
      format!("len={}", self.len()),
    );
  }
}

impl<K, V, S> DevtoolsInspectable for std::collections::HashMap<K, V, S> {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.value(
      std::any::type_name::<std::collections::HashMap<K, V, S>>(),
      format!("len={}", self.len()),
    );
  }
}

impl DevtoolsInspectable for str {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.value_debug(self);
  }
}

impl<T: DevtoolsInspectable + ?Sized> DevtoolsInspectable for Box<T> {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    (**self).write_info(formatter.buffer_mut());
  }
}

impl<T: SignalValue> DevtoolsInspectable for Signal<T> {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.value(std::any::type_name::<Signal<T>>(), format!("#{}", self.id()));
  }
}

impl DevtoolsInspectable for crate::core::NodeId {
  fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
    formatter.value(
      std::any::type_name::<crate::core::NodeId>(),
      format!("#{}", self.value()),
    );
  }
}

macro_rules! impl_tuple_devtools_inspectable {
  ($($name:ident:$index:tt),+ $(,)?) => {
    impl<$($name),+> DevtoolsInspectable for ($($name,)+)
    where
      $($name: Send + PartialEq + 'static),+
    {
      fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
        $(
          formatter.buffer_mut().push(ComponentInfo::new(stringify!($index), std::any::type_name::<$name>()));
        )+
      }
    }
  };
}

impl_tuple_devtools_inspectable!(A:0);
impl_tuple_devtools_inspectable!(A:0, B:1);
impl_tuple_devtools_inspectable!(A:0, B:1, C:2);
impl_tuple_devtools_inspectable!(A:0, B:1, C:2, D:3);
impl_tuple_devtools_inspectable!(A:0, B:1, C:2, D:3, E:4);
impl_tuple_devtools_inspectable!(A:0, B:1, C:2, D:3, E:4, F:5);

#[cfg(test)]
mod tests {
  use super::{ComponentInfo, DevtoolsFormatter, DevtoolsInspectable};

  struct FormatterDebugValue {
    count: u32,
    label: &'static str,
  }

  impl DevtoolsInspectable for FormatterDebugValue {
    fn inspect(&self, formatter: &mut DevtoolsFormatter<'_>) {
      formatter
        .debug_struct("FormatterDebugValue")
        .field("count", &self.count)
        .field_debug("label", &self.label)
        .finish();
    }
  }

  #[test]
  fn formatter_debug_struct_adds_named_fields() {
    let value = FormatterDebugValue {
      count: 7,
      label: "ready",
    };
    let mut fields = Vec::new();

    value.write_info(&mut fields);

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name(), "count");
    assert_eq!(fields[0].type_name(), "u32");
    assert_eq!(fields[0].formatted_value(), Some("7"));
    assert_eq!(fields[1].name(), "label");
    assert_eq!(fields[1].formatted_value(), Some("\"ready\""));
  }

  #[test]
  fn formatter_debug_map_allows_dynamic_keys() {
    let mut fields = Vec::new();
    let mut formatter = DevtoolsFormatter::new(&mut fields);

    formatter.debug_map().entry_debug("theme", &42_u32).finish();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name(), "\"theme\"");
    assert_eq!(fields[0].formatted_value(), Some("42"));
  }

  #[test]
  fn legacy_write_info_impls_still_work() {
    struct LegacyValue;

    impl DevtoolsInspectable for LegacyValue {
      fn write_info(&self, buffer: &mut Vec<ComponentInfo>) {
        buffer.push(ComponentInfo::with_value("legacy", "LegacyValue", "ok"));
      }
    }

    let mut fields = Vec::new();
    LegacyValue.write_info(&mut fields);

    assert_eq!(fields[0].name(), "legacy");
    assert_eq!(fields[0].formatted_value(), Some("ok"));
  }

  #[test]
  fn derive_uses_formatter_for_tuple_structs() {
    #[derive(crate::DevtoolsInspectable)]
    struct TupleValue(u32, &'static str);

    let mut fields = Vec::new();
    TupleValue(9, "items").write_info(&mut fields);

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name(), "0");
    assert_eq!(fields[0].formatted_value(), Some("9"));
    assert_eq!(fields[1].name(), "1");
    assert_eq!(fields[1].formatted_value(), Some("\"items\""));
  }
}

pub trait Component: Send + Sync + 'static {
  #[cfg(feature = "devtools")]
  type Props: Send + PartialEq + DevtoolsInspectable + 'static;
  #[cfg(not(feature = "devtools"))]
  type Props: Send + PartialEq + 'static;
  fn create(ctx: &mut Ctx) -> Self;
  fn render(&self, ctx: &mut Ctx) -> impl Into<Element>;
  fn after_layout(&self) {}
  fn on_mounted(&self) {}
  fn on_unmounted(&self) {}
}
