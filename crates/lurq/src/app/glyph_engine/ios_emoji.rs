//! Emoji on iOS, drawn by the system.
//!
//! Apple Color Emoji keeps its pictures in an `sbix` table. On macOS they are PNGs, which swash decodes; on
//! iOS they are `emjc`, a format of Apple's own that swash does not read, so every emoji was laid out with its
//! width and drew nothing. Core Text reads its own font: a glyph swash gives no picture for, in a font that
//! has an `sbix` table, is drawn by Core Text into a bitmap of the same shape swash would have given.

use std::ffi::{c_char, c_void};

use cosmic_text::{Font, SwashContent, SwashImage};
use swash::zeno::Placement;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGPoint {
  x: f64,
  y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGSize {
  width: f64,
  height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGRect {
  origin: CGPoint,
  size: CGSize,
}

type Ref = *const c_void;

/// `kCFStringEncodingUTF8`.
const UTF8: u32 = 0x0800_0100;
/// `kCGImageAlphaPremultipliedLast`: red, green, blue, alpha, the colours multiplied by the alpha.
const RGBA_PREMULTIPLIED: u32 = 1;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
  fn CFStringCreateWithCString(allocator: Ref, string: *const c_char, encoding: u32) -> Ref;
  fn CFRelease(object: Ref);
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
  fn CGColorSpaceCreateDeviceRGB() -> Ref;
  fn CGColorSpaceRelease(space: Ref);
  fn CGBitmapContextCreate(
    data: *mut c_void,
    width: usize,
    height: usize,
    bits_per_component: usize,
    bytes_per_row: usize,
    space: Ref,
    bitmap_info: u32,
  ) -> Ref;
  fn CGContextRelease(context: Ref);
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
  fn CTFontCreateWithName(name: Ref, size: f64, matrix: *const c_void) -> Ref;
  fn CTFontGetBoundingRectsForGlyphs(
    font: Ref,
    orientation: u32,
    glyphs: *const u16,
    bounding_rects: *mut CGRect,
    count: isize,
  ) -> CGRect;
  fn CTFontDrawGlyphs(font: Ref, glyphs: *const u16, positions: *const CGPoint, count: usize, context: Ref);
}

/// Whether the font keeps pictures of its glyphs in an `sbix` table, as Apple Color Emoji does.
pub(super) fn has_pictures(font: &Font) -> bool {
  font.as_swash().table(swash::tag_from_bytes(b"sbix")).is_some()
}

/// Whether swash drew nothing: no pixels, or pixels that are all clear, which is what it makes of a picture it
/// cannot read.
pub(super) fn is_blank(image: &SwashImage) -> bool {
  let pixel = match image.content {
    SwashContent::Color => 4,
    SwashContent::SubpixelMask => 3,
    SwashContent::Mask => 1,
  };
  image.placement.width == 0
    || image.placement.height == 0
    || image.data.chunks_exact(pixel).all(|pixel| pixel[pixel.len() - 1] == 0)
}

/// The glyph of Apple Color Emoji at `size` pixels, drawn by Core Text: straight RGBA, placed as swash places
/// a colour bitmap. `None` when the system has no such font or the glyph has no picture.
pub(super) fn draw(glyph: u16, size: f32) -> Option<SwashImage> {
  unsafe {
    let name = CFStringCreateWithCString(std::ptr::null(), c"AppleColorEmoji".as_ptr(), UTF8);
    if name.is_null() {
      return None;
    }
    let font = CTFontCreateWithName(name, f64::from(size), std::ptr::null());
    CFRelease(name);
    if font.is_null() {
      return None;
    }
    let image = draw_with(font, glyph);
    CFRelease(font);
    image
  }
}

unsafe fn draw_with(font: Ref, glyph: u16) -> Option<SwashImage> {
  // The glyph's box about its origin on the baseline, y up.
  let bounds = unsafe { CTFontGetBoundingRectsForGlyphs(font, 0, &glyph, std::ptr::null_mut(), 1) };
  let (left, bottom) = (bounds.origin.x.floor(), bounds.origin.y.floor());
  let width = ((bounds.origin.x + bounds.size.width).ceil() - left) as usize;
  let height = ((bounds.origin.y + bounds.size.height).ceil() - bottom) as usize;
  if width == 0 || height == 0 || width > 2048 || height > 2048 {
    return None;
  }
  let mut data = vec![0u8; width * height * 4];
  unsafe {
    let space = CGColorSpaceCreateDeviceRGB();
    let context = CGBitmapContextCreate(
      data.as_mut_ptr().cast(),
      width,
      height,
      8,
      width * 4,
      space,
      RGBA_PREMULTIPLIED,
    );
    CGColorSpaceRelease(space);
    if context.is_null() {
      return None;
    }
    // The context's origin is its bottom left, and its first row in memory is its top one.
    let position = CGPoint { x: -left, y: -bottom };
    CTFontDrawGlyphs(font, &glyph, &position, 1, context);
    CGContextRelease(context);
  }
  if data.chunks_exact(4).all(|pixel| pixel[3] == 0) {
    return None;
  }
  // Core Graphics multiplies the colours by the alpha; a glyph atlas keeps them straight.
  for pixel in data.chunks_exact_mut(4) {
    let alpha = u32::from(pixel[3]);
    if alpha != 0 && alpha != 255 {
      for channel in &mut pixel[..3] {
        *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
      }
    }
  }
  Some(SwashImage {
    source: swash::scale::Source::ColorBitmap(swash::scale::StrikeWith::BestFit),
    content: SwashContent::Color,
    placement: Placement {
      left: left as i32,
      top: (bottom as i32) + height as i32,
      width: width as u32,
      height: height as u32,
    },
    data,
  })
}
