mod guard;
mod handle;
mod navigator;
pub mod pattern;
pub(crate) mod query;
pub(crate) mod route_match;

use std::sync::Arc;

pub use guard::GuardAction;
pub use handle::RouterHandle;
pub(crate) use handle::StackEntry;
pub use navigator::Navigator;
pub use pattern::Pattern;
pub use query::Query;
pub use route_match::{Params, RouteMatch};

use crate::{app::ctx::Ctx, node::Element};

pub(crate) type RenderFn = Arc<dyn Fn(&mut Ctx) -> Element + Send + Sync>;

struct RouteDef {
  pattern: Pattern,
  kind: RouteDefKind,
  guard: Option<Arc<guard::GuardFn>>,
  index: usize,
}

enum RouteDefKind {
  Leaf(RenderFn),
  Layout { render: RenderFn, children: Vec<RouteDef> },
  Fallback(RenderFn),
}

pub struct Routes {
  defs: Vec<RouteDef>,
  next_index: usize,
}

impl Routes {
  pub fn new() -> Self {
    Self {
      defs: Vec::new(),
      next_index: 0,
    }
  }

  pub fn route(mut self, path: &str, render: impl Fn(&mut Ctx) -> Element + Send + Sync + 'static) -> Self {
    let index = self.next_index;
    self.next_index += 1;
    self.defs.push(RouteDef {
      pattern: Pattern::new(path),
      kind: RouteDefKind::Leaf(Arc::new(render)),
      guard: None,
      index,
    });
    self
  }

  pub fn layout(
    mut self,
    path: &str,
    render: impl Fn(&mut Ctx) -> Element + Send + Sync + 'static,
    children: impl FnOnce(Routes) -> Routes,
  ) -> Self {
    let index = self.next_index;
    self.next_index += 1;
    let child_routes = children(Routes {
      defs: Vec::new(),
      next_index: self.next_index,
    });
    self.next_index = child_routes.next_index;
    self.defs.push(RouteDef {
      pattern: Pattern::new(path),
      kind: RouteDefKind::Layout {
        render: Arc::new(render),
        children: child_routes.defs,
      },
      guard: None,
      index,
    });
    self
  }

  pub fn fallback(mut self, render: impl Fn(&mut Ctx) -> Element + Send + Sync + 'static) -> Self {
    let index = self.next_index;
    self.next_index += 1;
    self.defs.push(RouteDef {
      pattern: Pattern::new("/**__fallback"),
      kind: RouteDefKind::Fallback(Arc::new(render)),
      guard: None,
      index,
    });
    self
  }

  pub fn guard(mut self, guard_fn: impl Fn(&RouteMatch) -> GuardAction + Send + Sync + 'static) -> Self {
    if let Some(last) = self.defs.last_mut() {
      last.guard = Some(Arc::new(guard_fn));
    }
    self
  }

  pub fn resolve(&self, path: &str) -> Vec<RouteMatch> {
    let segments = pattern::normalize_segments(path);
    let path_only = path.split(['?', '#']).next().unwrap_or(path);
    let path_arc: Arc<str> = Arc::from(path_only);
    let mut chain = Vec::new();
    let mut params = Params::default();
    resolve_defs(&self.defs, &segments, &path_arc, &mut params, &mut chain);
    chain
  }

  pub(crate) fn guard_for(&self, route_index: usize) -> Option<&Arc<guard::GuardFn>> {
    find_guard_by_index(&self.defs, route_index)
  }
}

impl Default for Routes {
  fn default() -> Self {
    Self::new()
  }
}

fn find_guard_by_index(defs: &[RouteDef], target_index: usize) -> Option<&Arc<guard::GuardFn>> {
  for def in defs {
    if def.index == target_index {
      return def.guard.as_ref();
    }
    if let RouteDefKind::Layout { children, .. } = &def.kind {
      if let Some(g) = find_guard_by_index(children, target_index) {
        return Some(g);
      }
    }
  }
  None
}

fn resolve_defs(
  defs: &[RouteDef],
  segments: &[&str],
  path: &Arc<str>,
  accumulated_params: &mut Params,
  chain: &mut Vec<RouteMatch>,
) -> bool {
  let mut candidates: Vec<&RouteDef> = defs.iter().collect();
  candidates.sort_by(|a, b| {
    let a_fb = matches!(a.kind, RouteDefKind::Fallback(_));
    let b_fb = matches!(b.kind, RouteDefKind::Fallback(_));
    match (a_fb, b_fb) {
      (true, false) => std::cmp::Ordering::Greater,
      (false, true) => std::cmp::Ordering::Less,
      _ => b.pattern.priority().cmp(&a.pattern.priority()),
    }
  });

  for def in &candidates {
    match &def.kind {
      RouteDefKind::Leaf(render) => {
        if let Some(leaf_params) = def.pattern.matches_segments(segments) {
          let mut merged = accumulated_params.clone();
          merged.merge_from(&leaf_params);
          chain.push(RouteMatch {
            path: path.clone(),
            params: merged,
            route_index: def.index,
            pattern_raw: Arc::from(def.pattern.raw()),
            render: render.clone(),
          });
          return true;
        }
      }
      RouteDefKind::Layout { render, children } => {
        if let Some((prefix_params, consumed)) = def.pattern.match_prefix(segments) {
          let remaining = &segments[consumed..];
          let mut merged = accumulated_params.clone();
          merged.merge_from(&prefix_params);
          chain.push(RouteMatch {
            path: path.clone(),
            params: merged.clone(),
            route_index: def.index,
            pattern_raw: Arc::from(def.pattern.raw()),
            render: render.clone(),
          });
          if resolve_defs(children, remaining, path, &mut merged, chain) {
            return true;
          }
          chain.pop();
        }
      }
      RouteDefKind::Fallback(render) => {
        chain.push(RouteMatch {
          path: path.clone(),
          params: accumulated_params.clone(),
          route_index: def.index,
          pattern_raw: Arc::from("**"),
          render: render.clone(),
        });
        return true;
      }
    }
  }

  false
}
