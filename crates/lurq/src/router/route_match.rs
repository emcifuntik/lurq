use std::{collections::BTreeMap, str::FromStr, sync::Arc};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Params {
  inner: BTreeMap<Arc<str>, Arc<str>>,
}

impl Params {
  pub fn get(&self, name: &str) -> Option<&str> {
    self.inner.get(name).map(|v| v.as_ref())
  }

  pub fn get_parsed<T: FromStr>(&self, name: &str) -> Option<T> {
    self.get(name)?.parse().ok()
  }

  pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
    self.inner.iter().map(|(k, v)| (k.as_ref(), v.as_ref()))
  }

  pub fn is_empty(&self) -> bool {
    self.inner.is_empty()
  }

  pub fn len(&self) -> usize {
    self.inner.len()
  }

  pub fn from_pairs<const N: usize>(pairs: [(&str, &str); N]) -> Self {
    let mut inner = BTreeMap::new();
    for (k, v) in pairs {
      inner.insert(Arc::from(k), Arc::from(v));
    }
    Self { inner }
  }

  pub(crate) fn set(&mut self, name: Arc<str>, value: Arc<str>) {
    self.inner.insert(name, value);
  }

  pub(crate) fn merge_from(&mut self, other: &Params) {
    for (k, v) in &other.inner {
      self.inner.insert(k.clone(), v.clone());
    }
  }
}

impl std::hash::Hash for Params {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    for (k, v) in &self.inner {
      k.hash(state);
      v.hash(state);
    }
  }
}

#[derive(Clone)]
pub struct RouteMatch {
  pub(crate) path: Arc<str>,
  pub(crate) params: Params,
  pub(crate) route_index: usize,
  pub(crate) pattern_raw: Arc<str>,
  pub(crate) render: super::RenderFn,
}

impl std::fmt::Debug for RouteMatch {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("RouteMatch")
      .field("path", &self.path)
      .field("params", &self.params)
      .field("route_index", &self.route_index)
      .field("pattern_raw", &self.pattern_raw)
      .finish()
  }
}

impl PartialEq for RouteMatch {
  fn eq(&self, other: &Self) -> bool {
    self.path == other.path
      && self.params == other.params
      && self.route_index == other.route_index
      && self.pattern_raw == other.pattern_raw
  }
}

impl RouteMatch {
  pub fn pattern_raw(&self) -> &str {
    &self.pattern_raw
  }

  pub fn route_index(&self) -> usize {
    self.route_index
  }

  pub fn params(&self) -> &Params {
    &self.params
  }

  pub fn path(&self) -> &str {
    &self.path
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct OutletDepth(pub usize);

/// The state of the history entry a screen of a stack is for: `Ctx::route_state` reads it there, and not the
/// current entry's.
#[derive(Clone)]
pub(crate) struct StackState(pub Option<Arc<dyn std::any::Any + Send + Sync>>);

#[derive(Clone)]
pub(crate) struct RouterMatches(pub Arc<Vec<RouteMatch>>);

impl std::hash::Hash for RouterMatches {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    self.0.len().hash(state);
    for m in self.0.iter() {
      m.path.hash(state);
      m.route_index.hash(state);
      m.params.hash(state);
    }
  }
}
