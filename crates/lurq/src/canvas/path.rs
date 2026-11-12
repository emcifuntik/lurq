use std::{
  f32::consts::{FRAC_PI_2, TAU},
  hash::{DefaultHasher, Hash, Hasher},
  ops::Deref,
  sync::{Arc, OnceLock},
};

use tiny_skia::{Path, PathBuilder, PathSegment, Point};

use super::{CanvasError, FillRule, transform};
use crate::node::transform::Transform2D;

/// Direction of a canvas arc in the downward-positive canvas coordinate system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArcDirection {
  #[default]
  Clockwise,
  CounterClockwise,
}

/// Reusable geometry. Clones and submitted drawings are independent of later edits.
#[derive(Clone, Default, Debug)]
pub struct Path2D {
  builder: PathBuilder,
  finished: OnceLock<Option<Geometry>>,
  current: Option<Point>,
  start: Option<Point>,
  segments: usize,
}

pub(crate) const MAX_PATH_SEGMENTS: usize = 65_536;

impl Path2D {
  pub fn new() -> Self {
    Self::default()
  }
  pub fn is_empty(&self) -> bool {
    self.segments == 0
  }
  pub fn segment_count(&self) -> usize {
    self.segments
  }

  fn accepts(&self, values: &[f32]) -> bool {
    self.segments < MAX_PATH_SEGMENTS && values.iter().all(|v| v.is_finite())
  }

  pub fn move_to(&mut self, x: f32, y: f32) {
    if !self.accepts(&[x, y]) {
      return;
    }
    self.builder.move_to(x, y);
    self.finished.take();
    self.current = Some(Point::from_xy(x, y));
    self.start = self.current;
    self.segments += 1;
  }

  pub fn line_to(&mut self, x: f32, y: f32) {
    if !self.accepts(&[x, y]) {
      return;
    }
    if self.current.is_none() {
      self.move_to(x, y);
      return;
    }
    self.builder.line_to(x, y);
    self.finished.take();
    self.current = Some(Point::from_xy(x, y));
    self.segments += 1;
  }

  pub fn quadratic_curve_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
    if !self.accepts(&[cx, cy, x, y]) {
      return;
    }
    if self.current.is_none() {
      self.move_to(cx, cy);
    }
    self.builder.quad_to(cx, cy, x, y);
    self.finished.take();
    self.current = Some(Point::from_xy(x, y));
    self.segments += 1;
  }

  pub fn bezier_curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
    if !self.accepts(&[c1x, c1y, c2x, c2y, x, y]) {
      return;
    }
    if self.current.is_none() {
      self.move_to(c1x, c1y);
    }
    self.builder.cubic_to(c1x, c1y, c2x, c2y, x, y);
    self.finished.take();
    self.current = Some(Point::from_xy(x, y));
    self.segments += 1;
  }

  pub fn close_path(&mut self) {
    if self.current.is_none() || !self.accepts(&[]) {
      return;
    }
    self.builder.close();
    self.finished.take();
    self.current = self.start;
    self.segments += 1;
  }

  pub fn rect(&mut self, x: f32, y: f32, width: f32, height: f32) {
    if !self.accepts(&[x, y, width, height, x + width, y + height]) {
      return;
    }
    self.move_to(x, y);
    self.line_to(x + width, y);
    self.line_to(x + width, y + height);
    self.line_to(x, y + height);
    self.close_path();
  }

  /// Adds a rounded rectangle with a uniform radius, clamped to half its smaller side.
  pub fn round_rect(&mut self, x: f32, y: f32, width: f32, height: f32, radius: f32) -> Result<(), CanvasError> {
    if radius < 0.0 {
      return Err(CanvasError::InvalidGeometry);
    }
    if !self.accepts(&[x, y, width, height, radius]) {
      return Ok(());
    }
    if width < 0.0 || height < 0.0 {
      let mut path = Path2D::new();
      path.round_rect(0.0, 0.0, width.abs(), height.abs(), radius)?;
      self.add_path(
        &path,
        Transform2D::translate(x, y).then(&Transform2D::scale(width.signum(), height.signum())),
      );
      return Ok(());
    }
    let (w, h) = (width, height);
    let r = radius.min(w * 0.5).min(h * 0.5);
    if r == 0.0 {
      self.rect(x, y, w, h);
      return Ok(());
    }
    self.move_to(x + r, y);
    self.line_to(x + w - r, y);
    self.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0, ArcDirection::Clockwise)?;
    self.line_to(x + w, y + h - r);
    self.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2, ArcDirection::Clockwise)?;
    self.line_to(x + r, y + h);
    self.arc(x + r, y + h - r, r, FRAC_PI_2, 2.0 * FRAC_PI_2, ArcDirection::Clockwise)?;
    self.line_to(x, y + r);
    self.arc(
      x + r,
      y + r,
      r,
      2.0 * FRAC_PI_2,
      3.0 * FRAC_PI_2,
      ArcDirection::Clockwise,
    )?;
    self.close_path();
    Ok(())
  }

  pub fn arc(
    &mut self,
    x: f32,
    y: f32,
    radius: f32,
    start: f32,
    end: f32,
    direction: ArcDirection,
  ) -> Result<(), CanvasError> {
    self.ellipse(x, y, radius, radius, 0.0, start, end, direction)
  }

  #[allow(clippy::too_many_arguments)]
  pub fn ellipse(
    &mut self,
    x: f32,
    y: f32,
    rx: f32,
    ry: f32,
    rotation: f32,
    start: f32,
    end: f32,
    direction: ArcDirection,
  ) -> Result<(), CanvasError> {
    if rx < 0.0 || ry < 0.0 {
      return Err(CanvasError::InvalidGeometry);
    }
    if !self.accepts(&[x, y, rx, ry, rotation, start, end]) {
      return Ok(());
    }
    let delta = f64::from(end) - f64::from(start);
    let tau = f64::from(TAU);
    let sweep = match direction {
      ArcDirection::Clockwise if delta >= tau => TAU,
      ArcDirection::CounterClockwise if -delta >= tau => -TAU,
      ArcDirection::Clockwise => delta.rem_euclid(tau) as f32,
      ArcDirection::CounterClockwise => -(-delta).rem_euclid(tau) as f32,
    };
    let (sin, cos) = rotation.sin_cos();
    let point = |a: f32| {
      let (s, c) = a.sin_cos();
      (x + cos * rx * c - sin * ry * s, y + sin * rx * c + cos * ry * s)
    };
    let derivative = |a: f32| {
      let (s, c) = a.sin_cos();
      (-cos * rx * s - sin * ry * c, -sin * rx * s + cos * ry * c)
    };
    let start = start.rem_euclid(TAU);
    let p = point(start);
    self.line_to(p.0, p.1);
    let count = (sweep.abs() / FRAC_PI_2).ceil() as usize;
    for i in 0..count {
      let a = start + sweep * i as f32 / count as f32;
      let b = start + sweep * (i + 1) as f32 / count as f32;
      let k = 4.0 / 3.0 * ((b - a) * 0.25).tan();
      let (p0, p1, d0, d1) = (point(a), point(b), derivative(a), derivative(b));
      self.bezier_curve_to(
        p0.0 + k * d0.0,
        p0.1 + k * d0.1,
        p1.0 - k * d1.0,
        p1.1 - k * d1.1,
        p1.0,
        p1.1,
      );
    }
    Ok(())
  }

  pub fn arc_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, radius: f32) -> Result<(), CanvasError> {
    if radius < 0.0 {
      return Err(CanvasError::InvalidGeometry);
    }
    if !self.accepts(&[x1, y1, x2, y2, radius]) {
      return Ok(());
    }
    let Some(p0) = self.current else {
      self.move_to(x1, y1);
      return Ok(());
    };
    let (ax, ay, bx, by) = (p0.x - x1, p0.y - y1, x2 - x1, y2 - y1);
    let (alen, blen) = (ax.hypot(ay), bx.hypot(by));
    if radius == 0.0 || alen == 0.0 || blen == 0.0 {
      self.line_to(x1, y1);
      return Ok(());
    }
    let (ax, ay, bx, by) = (ax / alen, ay / alen, bx / blen, by / blen);
    let cross = ax * by - ay * bx;
    if cross.abs() < 1e-6 {
      self.line_to(x1, y1);
      return Ok(());
    }
    let angle = (ax * bx + ay * by).clamp(-1.0, 1.0).acos();
    let distance = radius / (angle * 0.5).tan();
    let (tx, ty) = (x1 + ax * distance, y1 + ay * distance);
    let sign = cross.signum();
    let (cx, cy) = (tx - ay * radius * sign, ty + ax * radius * sign);
    let end = (x1 + bx * distance, y1 + by * distance);
    self.line_to(tx, ty);
    self.arc(
      cx,
      cy,
      radius,
      (ty - cy).atan2(tx - cx),
      (end.1 - cy).atan2(end.0 - cx),
      if cross < 0.0 {
        ArcDirection::Clockwise
      } else {
        ArcDirection::CounterClockwise
      },
    )
  }

  pub fn add_path(&mut self, other: &Path2D, matrix: Transform2D) {
    if let Some(path) = other.finish().and_then(|p| p.transform(transform(matrix))) {
      self.append(&path, false);
    }
  }

  pub(crate) fn finish(&self) -> Option<Path> {
    self.builder.clone().finish()
  }
  pub(crate) fn geometry(&self) -> Option<Geometry> {
    self.finished.get_or_init(|| self.finish().map(Geometry::new)).clone()
  }
  pub(crate) fn current(&self) -> Option<Point> {
    self.current
  }

  pub(crate) fn append(&mut self, path: &Path, connect: bool) {
    let mut first = true;
    for segment in path.segments() {
      match segment {
        PathSegment::MoveTo(p) if first && connect => self.line_to(p.x, p.y),
        PathSegment::MoveTo(p) => self.move_to(p.x, p.y),
        PathSegment::LineTo(p) => self.line_to(p.x, p.y),
        PathSegment::QuadTo(c, p) => self.quadratic_curve_to(c.x, c.y, p.x, p.y),
        PathSegment::CubicTo(a, b, p) => self.bezier_curve_to(a.x, a.y, b.x, b.y, p.x, p.y),
        PathSegment::Close => self.close_path(),
      }
      first = false;
    }
  }
}

/// Immutable, compact snapshot with a precomputed content hash. Equal rebuilt
/// paths share meshes too; equality checks the geometry, never just the hash.
#[derive(Clone, Debug)]
pub(crate) struct Geometry(Arc<GeometryData>);
#[derive(Debug)]
struct GeometryData {
  path: Path,
  hash: u64,
  curves: bool,
}
impl Geometry {
  pub fn new(path: Path) -> Self {
    let mut hash = DefaultHasher::new();
    for verb in path.verbs() {
      (*verb as u8).hash(&mut hash);
    }
    for p in path.points() {
      for v in [p.x, p.y] {
        // Path equality treats signed zero as equal.
        (if v == 0. { 0 } else { v.to_bits() }).hash(&mut hash);
      }
    }
    let curves = path
      .segments()
      .any(|v| matches!(v, PathSegment::QuadTo(..) | PathSegment::CubicTo(..)));
    // Clone trims spare builder/stroker capacity before the cache retains it.
    Self(Arc::new(GeometryData {
      path: path.clone(),
      hash: hash.finish(),
      curves,
    }))
  }
  pub fn bytes(&self) -> usize {
    std::mem::size_of_val(self.points()) + self.verbs().len() + std::mem::size_of::<GeometryData>()
  }
  pub fn has_curves(&self) -> bool {
    self.0.curves
  }
  /// The precomputed content hash. Effects key their rasters on it.
  pub fn content_hash(&self) -> u64 {
    self.0.hash
  }
}
impl Deref for Geometry {
  type Target = Path;
  fn deref(&self) -> &Path {
    &self.0.path
  }
}
impl PartialEq for Geometry {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0) || self.0.path == other.0.path
  }
}
impl Eq for Geometry {}
impl Hash for Geometry {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.0.hash.hash(state);
  }
}

/// Winding queries operate on vector geometry, independent of clipping and raster alpha.
pub(crate) fn contains(path: &Path, point: Point, rule: FillRule) -> bool {
  if !point.x.is_finite() || !point.y.is_finite() {
    return false;
  }
  let mut winding = 0i32;
  let mut boundary = false;
  let mut edge = |a: Point, b: Point| {
    let cross = f64::from(b.x - a.x) * f64::from(point.y - a.y) - f64::from(b.y - a.y) * f64::from(point.x - a.x);
    if cross.abs() < 1e-5
      && point.x >= a.x.min(b.x)
      && point.x <= a.x.max(b.x)
      && point.y >= a.y.min(b.y)
      && point.y <= a.y.max(b.y)
    {
      boundary = true;
    }
    if a.y <= point.y && b.y > point.y && cross > 0.0 {
      winding += 1;
    }
    if a.y > point.y && b.y <= point.y && cross < 0.0 {
      winding -= 1;
    }
  };
  let (mut current, mut start) = (None, None);
  for segment in path.segments() {
    match segment {
      PathSegment::MoveTo(p) => {
        if let (Some(a), Some(b)) = (current, start) {
          edge(a, b);
        }
        current = Some(p);
        start = Some(p);
      }
      PathSegment::LineTo(p) => {
        if let Some(a) = current {
          edge(a, p);
        }
        current = Some(p);
      }
      PathSegment::QuadTo(c, p) => {
        if let Some(a) = current {
          flatten(a, lerp(a, c, 2.0 / 3.0), lerp(p, c, 2.0 / 3.0), p, 0, &mut edge);
        }
        current = Some(p);
      }
      PathSegment::CubicTo(a, b, p) => {
        if let Some(c) = current {
          flatten(c, a, b, p, 0, &mut edge);
        }
        current = Some(p);
      }
      PathSegment::Close => {
        if let (Some(a), Some(b)) = (current, start) {
          edge(a, b);
        }
        current = start;
      }
    }
  }
  if let (Some(a), Some(b)) = (current, start) {
    edge(a, b);
  }
  boundary
    || match rule {
      FillRule::NonZero => winding != 0,
      FillRule::EvenOdd => winding.abs() % 2 == 1,
    }
}

fn lerp(a: Point, b: Point, t: f32) -> Point {
  Point::from_xy(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}
fn flatten(a: Point, b: Point, c: Point, d: Point, depth: u8, edge: &mut impl FnMut(Point, Point)) {
  let deviation = (b.x - (2.0 * a.x + d.x) / 3.0)
    .hypot(b.y - (2.0 * a.y + d.y) / 3.0)
    .max((c.x - (a.x + 2.0 * d.x) / 3.0).hypot(c.y - (a.y + 2.0 * d.y) / 3.0));
  if depth >= 12 || deviation < 0.01 {
    edge(a, d);
    return;
  }
  let (ab, bc, cd) = (lerp(a, b, 0.5), lerp(b, c, 0.5), lerp(c, d, 0.5));
  let (abc, bcd) = (lerp(ab, bc, 0.5), lerp(bc, cd, 0.5));
  let mid = lerp(abc, bcd, 0.5);
  flatten(a, ab, abc, mid, depth + 1, edge);
  flatten(mid, bcd, cd, d, depth + 1, edge);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn snapshots_invalidate_on_each_mutation_and_hash_collisions_check_content() {
    let mut p = Path2D::new();
    assert!(p.geometry().is_none());
    p.move_to(0., 0.);
    p.line_to(10., 10.);
    let mut previous = p.geometry().unwrap();
    for mutation in 0..5 {
      match mutation {
        0 => p.move_to(20., 20.),
        1 => p.line_to(30., 20.),
        2 => p.quadratic_curve_to(40., 30., 30., 40.),
        3 => p.bezier_curve_to(20., 40., 10., 30., 10., 20.),
        _ => p.close_path(),
      }
      let next = p.geometry().unwrap();
      assert_ne!(previous, next);
      previous = next;
    }
    let mut different = Path2D::new();
    different.rect(0., 0., 5., 5.);
    let collision = Geometry(Arc::new(GeometryData {
      path: different.finish().unwrap(),
      hash: previous.0.hash,
      curves: false,
    }));
    let mut map = std::collections::HashMap::new();
    map.insert(previous.clone(), 1);
    map.insert(collision.clone(), 2);
    assert_eq!(map[&previous], 1);
    assert_eq!(map[&collision], 2);
  }
}
