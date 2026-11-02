use std::{
  collections::{HashMap, VecDeque},
  sync::{Arc, Mutex},
};

use crate::node::color::Color;

/// Parsed trees interned by content hash: re-creating an `SvgData` from the
/// same source (a common per-render pattern in apps) skips the usvg parse
/// and keeps the id stable, which is what lets the raster and GPU caches
/// hit across instances.
static TREE_CACHE: Mutex<Option<TreeCache>> = Mutex::new(None);

const TREE_CACHE_CAP: usize = 256;

#[derive(Default)]
struct TreeCache {
  map: HashMap<u64, Arc<usvg::Tree>>,
  order: VecDeque<u64>,
}

/// FNV-1a over the raw bytes — deterministic within and across processes.
fn content_id(data: &[u8]) -> u64 {
  let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
  for byte in data {
    hash ^= u64::from(*byte);
    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
  }
  hash
}

/// Fold an override into the id deterministically (splitmix64 finalizer), so
/// identical construction chains produce identical ids.
fn mix_id(id: u64, tag: u64, value: u64) -> u64 {
  let mut x = id ^ tag.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
  x ^= x >> 30;
  x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
  x ^= x >> 27;
  x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
  x ^ (x >> 31)
}

fn color_bits(color: Color) -> u64 {
  let [r, g, b, a] = [color.r(), color.g(), color.b(), color.a()].map(u64::from);
  r << 48 | g << 32 | b << 16 | a
}

#[derive(Clone)]
pub struct SvgData {
  id: u64,
  tree: Arc<usvg::Tree>,
  overrides: SvgOverrides,
}

#[derive(Clone, Default)]
struct SvgOverrides {
  fill: Option<Color>,
  stroke: Option<Color>,
  opacity: Option<f32>,
}

impl SvgData {
  pub fn from_bytes(data: &[u8]) -> Self {
    let id = content_id(data);
    let mut guard = TREE_CACHE.lock().unwrap();
    let cache = guard.get_or_insert_with(TreeCache::default);
    let tree = if let Some(tree) = cache.map.get(&id) {
      tree.clone()
    } else {
      let tree = Arc::new(usvg::Tree::from_data(data, &usvg::Options::default()).expect("invalid SVG data"));
      if cache.order.len() >= TREE_CACHE_CAP
        && let Some(evicted) = cache.order.pop_front()
      {
        cache.map.remove(&evicted);
      }
      cache.map.insert(id, tree.clone());
      cache.order.push_back(id);
      tree
    };
    drop(guard);
    Self {
      id,
      tree,
      overrides: SvgOverrides::default(),
    }
  }

  pub fn from_str(svg: &str) -> Self {
    Self::from_bytes(svg.as_bytes())
  }

  pub fn with_fill(mut self, color: Color) -> Self {
    self.overrides.fill = Some(color);
    self.id = mix_id(self.id, 1, color_bits(color));
    self
  }

  pub fn with_stroke(mut self, color: Color) -> Self {
    self.overrides.stroke = Some(color);
    self.id = mix_id(self.id, 2, color_bits(color));
    self
  }

  pub fn with_opacity(mut self, opacity: f32) -> Self {
    let opacity = opacity.clamp(0.0, 1.0);
    self.overrides.opacity = Some(opacity);
    self.id = mix_id(self.id, 3, u64::from(opacity.to_bits()));
    self
  }

  pub fn id(&self) -> u64 {
    self.id
  }

  pub fn tree(&self) -> &usvg::Tree {
    &self.tree
  }

  pub fn fill_override(&self) -> Option<Color> {
    self.overrides.fill
  }

  pub fn stroke_override(&self) -> Option<Color> {
    self.overrides.stroke
  }

  pub fn opacity_override(&self) -> Option<f32> {
    self.overrides.opacity
  }

  pub fn viewbox_width(&self) -> f32 {
    self.tree.size().width()
  }

  pub fn viewbox_height(&self) -> f32 {
    self.tree.size().height()
  }
}
