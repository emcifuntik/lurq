//! SVG identity and caching: ids are content-derived (stable across
//! re-parsing the same source, the common per-render pattern), and repeated
//! layout of the same SVG at the same size must not re-rasterize — an
//! uncached full-viewport layer cost 40-70ms EVERY frame.
#![cfg(feature = "svg")]

use lurq::svg::SvgData;

const RING: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="60">
  <path d="M10 10 L50 10 L50 50 L10 50 Z" fill="#3fa7d6" stroke="#173753" stroke-width="2"/>
</svg>"##;

const OTHER: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="60">
  <circle cx="30" cy="30" r="20" fill="#d63f3f"/>
</svg>"##;

#[test]
fn ids_are_content_stable_across_instances() {
  assert_eq!(SvgData::from_str(RING).id(), SvgData::from_str(RING).id());
  assert_ne!(SvgData::from_str(RING).id(), SvgData::from_str(OTHER).id());
}

#[test]
fn override_chains_derive_deterministic_distinct_ids() {
  use lurq::node::color::Color;
  let plain = SvgData::from_str(RING);
  let tinted_a = SvgData::from_str(RING).with_fill(Color::from_hex("#ffcc00"));
  let tinted_b = SvgData::from_str(RING).with_fill(Color::from_hex("#ffcc00"));
  let other_tint = SvgData::from_str(RING).with_fill(Color::from_hex("#00ccff"));
  assert_eq!(tinted_a.id(), tinted_b.id());
  assert_ne!(tinted_a.id(), plain.id());
  assert_ne!(tinted_a.id(), other_tint.id());
}

#[test]
fn interned_trees_are_shared_across_instances() {
  let first = SvgData::from_str(RING);
  let second = SvgData::from_str(RING);
  assert!(std::ptr::eq(first.tree(), second.tree()));
}
