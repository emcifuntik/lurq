//! The platform's own symbols as images: the arrow of a way back, as UIKit draws it. A symbol is a template,
//! a shape with no color of its own, drawn here in the color asked for.

use std::{collections::HashMap, ffi::c_void, sync::Mutex};

use objc2::{
  class,
  encode::{Encode, Encoding},
  msg_send,
  rc::Retained,
  runtime::AnyObject,
};

use crate::{images::ImageData, node::color::Color};

type Ref = *const c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
  width: f64,
  height: f64,
}

unsafe impl Encode for CGSize {
  const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
  x: f64,
  y: f64,
  width: f64,
  height: f64,
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
  fn CGContextDrawImage(context: Ref, rect: CGRect, image: Ref);
  fn CGContextRelease(context: Ref);
}

/// `kCGImageAlphaPremultipliedLast`: RGBA, the color already multiplied by the alpha.
const PREMULTIPLIED_LAST: u32 = 1;
/// `UIImageSymbolWeightSemibold` and `UIImageSymbolScaleLarge`: how a navigation bar draws its symbols, beside
/// text of the same point size.
const SEMIBOLD: isize = 6;
const LARGE: isize = 3;
/// The pixels drawn for each point: a phone's densest display.
const SCALE: f64 = 3.0;

/// A symbol and how large it is, in logical pixels.
#[derive(Clone)]
pub(crate) struct Symbol {
  pub(crate) image: ImageData,
  pub(crate) width: f32,
  pub(crate) height: f32,
}

/// The system's symbol of a name, semibold and large for a point size, in a color; `None` where the system has none of
/// that name. Drawn once for each.
pub(crate) fn symbol(name: &str, point_size: f32, color: Color) -> Option<Symbol> {
  static DRAWN: Mutex<Option<HashMap<(String, u32, [u8; 3]), Option<Symbol>>>> = Mutex::new(None);
  let key = (name.to_owned(), point_size.to_bits(), [color.r(), color.g(), color.b()]);
  let mut drawn = DRAWN.lock().unwrap();
  drawn
    .get_or_insert_with(HashMap::new)
    .entry(key)
    .or_insert_with(|| draw(name, point_size, color))
    .clone()
}

fn draw(name: &str, point_size: f32, color: Color) -> Option<Symbol> {
  let units: Vec<u16> = name.encode_utf16().collect();
  unsafe {
    let name: Retained<AnyObject> =
      msg_send![class!(NSString), stringWithCharacters: units.as_ptr(), length: units.len()];
    let configuration: Retained<AnyObject> = msg_send![
      class!(UIImageSymbolConfiguration),
      configurationWithPointSize: f64::from(point_size),
      weight: SEMIBOLD,
      scale: LARGE
    ];
    let image: Option<Retained<AnyObject>> =
      msg_send![class!(UIImage), systemImageNamed: &*name, withConfiguration: &*configuration];
    let image = image?;
    let size: CGSize = msg_send![&*image, size];
    let shape: Ref = msg_send![&*image, CGImage];
    if shape.is_null() || size.width <= 0.0 || size.height <= 0.0 {
      return None;
    }
    let (width, height) = (
      (size.width * SCALE).ceil() as usize,
      (size.height * SCALE).ceil() as usize,
    );
    let mut pixels = vec![0u8; width * height * 4];
    let space = CGColorSpaceCreateDeviceRGB();
    let context = CGBitmapContextCreate(
      pixels.as_mut_ptr().cast(),
      width,
      height,
      8,
      width * 4,
      space,
      PREMULTIPLIED_LAST,
    );
    CGColorSpaceRelease(space);
    if context.is_null() {
      return None;
    }
    CGContextDrawImage(
      context,
      CGRect {
        x: 0.0,
        y: 0.0,
        width: width as f64,
        height: height as f64,
      },
      shape,
    );
    CGContextRelease(context);
    // The shape is in the alpha; the color is the one asked for.
    let (red, green, blue) = (color.r(), color.g(), color.b());
    for pixel in pixels.chunks_exact_mut(4) {
      pixel[0] = red;
      pixel[1] = green;
      pixel[2] = blue;
    }
    Some(Symbol {
      image: ImageData::from_rgba(pixels, width as u32, height as u32),
      width: size.width as f32,
      height: size.height as f32,
    })
  }
}
