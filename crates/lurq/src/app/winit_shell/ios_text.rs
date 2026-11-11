//! Text input on iOS: the view the keyboard types into, which is the focused text input as UIKit sees it.
//!
//! winit shows the keyboard through a view of its own that is told keys one at a time and tells nothing back,
//! so the keyboard knew nothing of the text: no autocorrection, no dictation, no composition, a delete key
//! that never speeds up, a password's keyboard recorded with the screen. [`TextInput`] is a view of the
//! shell's own that takes the keyboard instead and speaks `UITextInput`, the protocol of a text editor: UIKit
//! asks it what the text is, where the selection and the caret are, and tells it to replace a range, move the
//! selection or mark text that is being composed.
//!
//! UIKit asks in the middle of anything, while the shell may be using the tree, so the view does not reach
//! the tree. It answers from a copy of the input ([`Mirror`]) and applies UIKit's edits to that copy at once,
//! so that the next question finds them made; the edits are also queued, and the shell makes them in the tree
//! on its next turn ([`TextInput::sync`]). Then it compares: when the tree's input is not what the copy says
//! (a handler refused the edit, the app set the value, a mouse moved the caret, another input took the focus),
//! the copy takes the tree's and UIKit is told the text or the selection changed, which makes it drop what it
//! had worked out about the old one.
//!
//! UIKit counts text in UTF-16 units; the tree counts bytes of UTF-8. The copy keeps the text as a `String`
//! and every position UIKit is given or gives back is converted.
//!
//! Where characters are on the screen is in the copy too: every place a caret can be, as the tree last painted
//! the input. From it UIKit is told where a position is, which rectangles a range covers and which position is
//! nearest a point.
//!
//! With that, UIKit's text interaction is put on the view: the platform's own caret, selection with handles to
//! drag, the loupe, and the menu to copy, cut, paste and select all. The view lies over the focused input, so
//! that their touches land on it, and the tree paints no caret of its own
//! (`Tree::set_native_text_selection`). The selection's highlight stays the tree's, under the text, and the
//! platform's is not shown. A touch the interaction does not take goes on up to winit's view, as
//! any view passes on what it does not handle: scrolling from an input still scrolls.

use std::{
  ffi::c_void,
  ptr::NonNull,
  sync::{
    Mutex,
    atomic::{AtomicPtr, Ordering},
  },
};

use objc2::{
  class,
  encode::{Encode, Encoding},
  msg_send,
  rc::Retained,
  runtime::{AnyClass, AnyObject, AnyProtocol, Bool, ClassBuilder, Sel},
  sel,
};

use super::ios::{CGPoint, CGRect, CGSize};
use crate::{
  app::{
    Tree,
    events::ImeEvent,
    runtime::{KeyboardTraits, TextInputSnapshot},
  },
  node::{
    node_kind::{ContentKind, KeyboardKind, ReturnKey},
    text_selection::previous_delete_boundary,
  },
};

#[repr(C)]
#[derive(Clone, Copy)]
struct NSRange {
  location: usize,
  length: usize,
}

unsafe impl Encode for NSRange {
  const ENCODING: Encoding = Encoding::Struct("_NSRange", &[usize::ENCODING, usize::ENCODING]);
}

/// What UIKit did to the text, for the shell to do in the tree. Offsets are bytes of the copy's text as it was
/// when the edit was made.
enum Edit {
  /// Typed text, whole: a letter, a word of dictation, an emoji with all its parts, a line break for Return.
  Insert(String),
  DeleteBackward,
  /// A range replaced: an autocorrection, a suggestion taken, a paste.
  Replace {
    start: usize,
    end: usize,
    text: String,
  },
  Select {
    start: usize,
    end: usize,
  },
  /// Text being composed, and the selection inside it.
  Mark {
    text: String,
    selection: Option<(usize, usize)>,
  },
  /// The composition is confirmed as this text.
  Unmark(String),
}

/// The focused text input as UIKit was last told it is.
#[derive(Default)]
struct Mirror {
  text: String,
  /// The selection, the start first, in bytes.
  selection: (usize, usize),
  /// The text being composed, in bytes.
  marked: Option<(usize, usize)>,
  /// Where the input and its characters are; `None` before its first frame.
  geometry: Option<Geometry>,
  traits: Option<KeyboardTraits>,
  edits: Vec<Edit>,
}

/// Where the focused input is on the screen, in points in the window.
#[derive(Clone, PartialEq)]
struct Geometry {
  /// The input's box, which is the view's frame: x, y, width, height.
  bounds: (f64, f64, f64, f64),
  /// Every place a caret can be: its byte in the text and its top-left corner.
  carets: Vec<(usize, f64, f64)>,
  line_height: f64,
}

/// How wide the caret is, in points.
const CARET_WIDTH: f64 = 2.0;

impl Geometry {
  /// Where a caret at the byte is: there, or at the last place before it.
  fn place(&self, byte: usize) -> (f64, f64) {
    self
      .carets
      .iter()
      .rev()
      .find(|(index, ..)| *index <= byte)
      .or(self.carets.first())
      .map_or((self.bounds.0, self.bounds.1), |(_, x, y)| (*x, *y))
  }

  /// A rectangle in the window as the view sees it: its frame is the input's box.
  fn in_view(&self, x: f64, y: f64, width: f64) -> CGRect {
    CGRect {
      origin: CGPoint {
        x: x - self.bounds.0,
        y: y - self.bounds.1,
      },
      size: CGSize {
        width,
        height: self.line_height,
      },
    }
  }

  /// The rectangles a range of bytes covers, one for each line it is on.
  fn rows(&self, start: usize, end: usize) -> Vec<(f64, f64, f64)> {
    let mut rows: Vec<(f64, f64, f64)> = Vec::new();
    for (_, x, y) in self.carets.iter().filter(|(index, ..)| (start..=end).contains(index)) {
      match rows.iter_mut().find(|(_, row, _)| (*row - *y).abs() < 0.5) {
        Some((left, _, right)) => {
          *left = left.min(*x);
          *right = right.max(*x);
        }
        None => rows.push((*x, *y, *x)),
      }
    }
    rows
  }

  /// The byte whose caret is nearest a point of the view: on the line the point is level with, the nearest
  /// across.
  fn nearest(&self, point: CGPoint) -> usize {
    let (x, y) = (point.x + self.bounds.0, point.y + self.bounds.1);
    let level = |row: f64| (row + self.line_height / 2.0 - y).abs();
    let Some(row) = self
      .carets
      .iter()
      .map(|(_, _, row)| *row)
      .min_by(|a, b| level(*a).total_cmp(&level(*b)))
    else {
      return 0;
    };
    self
      .carets
      .iter()
      .filter(|(_, _, caret)| (*caret - row).abs() < 0.5)
      .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
      .map_or(0, |(index, ..)| *index)
  }
}

static MIRROR: Mutex<Option<Mirror>> = Mutex::new(None);
/// The window's view, which the text input's view is in and outlives it. Asking it to draw wakes the shell.
static WINDOW_VIEW: AtomicPtr<AnyObject> = AtomicPtr::new(std::ptr::null_mut());
/// The rectangles UIKit last showed the selection as, in the window, as the tree was told them.
static HIGHLIGHT: Mutex<Vec<(f64, f64, f64, f64)>> = Mutex::new(Vec::new());

/// UIKit's delegate, which is told of changes it did not make. UIKit owns it and clears it.
static INPUT_DELEGATE: AtomicPtr<AnyObject> = AtomicPtr::new(std::ptr::null_mut());
/// The tokenizer UIKit asks for word and line boundaries, made once for the view.
static TOKENIZER: AtomicPtr<AnyObject> = AtomicPtr::new(std::ptr::null_mut());

fn with_mirror<T>(default: T, read: impl FnOnce(&mut Mirror) -> T) -> T {
  MIRROR.lock().unwrap().as_mut().map_or(default, read)
}

impl Mirror {
  fn utf16_len(&self) -> isize {
    self.text.encode_utf16().count() as isize
  }

  /// The byte offset of a UTF-16 offset, which is brought into the text and onto a character.
  fn byte_of(&self, utf16: isize) -> usize {
    let mut units = 0isize;
    for (byte, character) in self.text.char_indices() {
      if units >= utf16 {
        return byte;
      }
      units += character.len_utf16() as isize;
    }
    self.text.len()
  }

  fn utf16_of(&self, byte: usize) -> isize {
    let byte = byte.min(self.text.len());
    self.text[..floor_char_boundary(&self.text, byte)]
      .encode_utf16()
      .count() as isize
  }

  /// Replaces bytes with text and puts the caret after it. Nothing is being composed afterwards.
  fn replace(&mut self, start: usize, end: usize, text: &str) {
    let start = floor_char_boundary(&self.text, start.min(self.text.len()));
    let end = floor_char_boundary(&self.text, end.clamp(start, self.text.len()));
    self.text.replace_range(start..end, text);
    self.selection = (start + text.len(), start + text.len());
    self.marked = None;
  }
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
  index = index.min(text.len());
  while index > 0 && !text.is_char_boundary(index) {
    index -= 1;
  }
  index
}

fn string_of(text: *mut AnyObject) -> Option<String> {
  let text = unsafe { text.as_ref() }?;
  let utf8: *const std::ffi::c_char = unsafe { msg_send![text, UTF8String] };
  if utf8.is_null() {
    return None;
  }
  Some(unsafe { std::ffi::CStr::from_ptr(utf8) }.to_string_lossy().into_owned())
}

/// An autoreleased `NSString`.
fn ns_string(text: &str) -> *mut AnyObject {
  let units: Vec<u16> = text.encode_utf16().collect();
  let string: Retained<AnyObject> =
    unsafe { msg_send![class!(NSString), stringWithCharacters: units.as_ptr(), length: units.len()] };
  Retained::autorelease_ptr(string)
}

// Positions and ranges: UIKit's are opaque objects that the editor makes. These hold UTF-16 offsets.

fn position_class() -> &'static AnyClass {
  if let Some(class) = AnyClass::get(c"LurqTextPosition") {
    return class;
  }
  let mut builder = ClassBuilder::new(c"LurqTextPosition", class!(UITextPosition)).expect("a class name of its own");
  builder.add_ivar::<isize>(c"lurqOffset");
  builder.register()
}

fn range_class() -> &'static AnyClass {
  if let Some(class) = AnyClass::get(c"LurqTextRange") {
    return class;
  }
  extern "C-unwind" fn start(this: &AnyObject, _cmd: Sel) -> *mut AnyObject {
    position(range_ends(this).0)
  }
  extern "C-unwind" fn end(this: &AnyObject, _cmd: Sel) -> *mut AnyObject {
    position(range_ends(this).1)
  }
  extern "C-unwind" fn is_empty(this: &AnyObject, _cmd: Sel) -> Bool {
    let (start, end) = range_ends(this);
    Bool::new(start == end)
  }
  let mut builder = ClassBuilder::new(c"LurqTextRange", class!(UITextRange)).expect("a class name of its own");
  builder.add_ivar::<isize>(c"lurqStart");
  builder.add_ivar::<isize>(c"lurqEnd");
  unsafe {
    builder.add_method(sel!(start), start as extern "C-unwind" fn(_, _) -> _);
    builder.add_method(sel!(end), end as extern "C-unwind" fn(_, _) -> _);
    builder.add_method(sel!(isEmpty), is_empty as extern "C-unwind" fn(_, _) -> _);
  }
  builder.register()
}

fn set_ivar(object: &AnyObject, class: &AnyClass, name: &std::ffi::CStr, value: isize) {
  let ivar = class.instance_variable(name).expect("an ivar the class was made with");
  unsafe { *ivar.load_ptr::<isize>(object) = value };
}

fn ivar(object: &AnyObject, class: &AnyClass, name: &std::ffi::CStr) -> isize {
  let ivar = class.instance_variable(name).expect("an ivar the class was made with");
  unsafe { *ivar.load::<isize>(object) }
}

/// An autoreleased position at a UTF-16 offset.
fn position(offset: isize) -> *mut AnyObject {
  let class = position_class();
  let position: Retained<AnyObject> = unsafe { msg_send![class, new] };
  set_ivar(&position, class, c"lurqOffset", offset);
  Retained::autorelease_ptr(position)
}

/// An autoreleased range between UTF-16 offsets, the smaller first.
fn range(start: isize, end: isize) -> *mut AnyObject {
  let class = range_class();
  let range: Retained<AnyObject> = unsafe { msg_send![class, new] };
  set_ivar(&range, class, c"lurqStart", start.min(end));
  set_ivar(&range, class, c"lurqEnd", start.max(end));
  Retained::autorelease_ptr(range)
}

/// The offset of a position UIKit gives back, when it is one of ours.
fn offset_of(position: *mut AnyObject) -> Option<isize> {
  let position = unsafe { position.as_ref() }?;
  let class = position_class();
  let ours: bool = unsafe { msg_send![position, isKindOfClass: class] };
  ours.then(|| ivar(position, class, c"lurqOffset"))
}

fn range_ends(range: &AnyObject) -> (isize, isize) {
  let class = range_class();
  (ivar(range, class, c"lurqStart"), ivar(range, class, c"lurqEnd"))
}

fn ends_of(range: *mut AnyObject) -> Option<(isize, isize)> {
  let range = unsafe { range.as_ref() }?;
  let ours: bool = unsafe { msg_send![range, isKindOfClass: range_class()] };
  ours.then(|| range_ends(range))
}

// What UIKit asks and tells: `UIKeyInput` and `UITextInput`.

extern "C-unwind" fn can_become_first_responder(_this: &AnyObject, _cmd: Sel) -> Bool {
  Bool::YES
}

extern "C-unwind" fn has_text(_this: &AnyObject, _cmd: Sel) -> Bool {
  Bool::new(with_mirror(false, |mirror| !mirror.text.is_empty()))
}

extern "C-unwind" fn insert_text(_this: &AnyObject, _cmd: Sel, text: *mut AnyObject) {
  let Some(text) = string_of(text) else {
    return;
  };
  with_mirror((), |mirror| {
    // Over what is being composed, or over the selection. A line break is the Return key: what it does is
    // the input's to say, and the copy is put right when the tree has done it.
    let (start, end) = mirror.marked.unwrap_or(mirror.selection);
    if !text.contains('\n') {
      mirror.replace(start, end, &text);
    }
    mirror.edits.push(Edit::Insert(text));
  });
}

extern "C-unwind" fn delete_backward(_this: &AnyObject, _cmd: Sel) {
  with_mirror((), |mirror| {
    let (start, end) = mirror.selection;
    if start != end {
      mirror.replace(start, end, "");
    } else if start > 0 {
      let from = previous_delete_boundary(&mirror.text, start);
      mirror.replace(from, start, "");
    }
    mirror.edits.push(Edit::DeleteBackward);
  });
}

extern "C-unwind" fn text_in_range(_this: &AnyObject, _cmd: Sel, range: *mut AnyObject) -> *mut AnyObject {
  let Some((start, end)) = ends_of(range) else {
    return std::ptr::null_mut();
  };
  let text = with_mirror(String::new(), |mirror| {
    mirror.text[mirror.byte_of(start)..mirror.byte_of(end)].to_owned()
  });
  ns_string(&text)
}

extern "C-unwind" fn replace_range(_this: &AnyObject, _cmd: Sel, range: *mut AnyObject, text: *mut AnyObject) {
  let (Some((start, end)), Some(text)) = (ends_of(range), string_of(text)) else {
    return;
  };
  with_mirror((), |mirror| {
    let (start, end) = (mirror.byte_of(start), mirror.byte_of(end));
    mirror.replace(start, end, &text);
    mirror.edits.push(Edit::Replace { start, end, text });
  });
}

extern "C-unwind" fn selected_text_range(_this: &AnyObject, _cmd: Sel) -> *mut AnyObject {
  match with_mirror(None, |mirror| {
    Some((mirror.utf16_of(mirror.selection.0), mirror.utf16_of(mirror.selection.1)))
  }) {
    Some((start, end)) => range(start, end),
    None => std::ptr::null_mut(),
  }
}

extern "C-unwind" fn set_selected_text_range(_this: &AnyObject, _cmd: Sel, selected: *mut AnyObject) {
  let Some((start, end)) = ends_of(selected) else {
    return;
  };
  with_mirror((), |mirror| {
    let selection = (mirror.byte_of(start), mirror.byte_of(end));
    if selection != mirror.selection {
      mirror.selection = selection;
      mirror.edits.push(Edit::Select {
        start: selection.0,
        end: selection.1,
      });
    }
  });
}

extern "C-unwind" fn marked_text_range(_this: &AnyObject, _cmd: Sel) -> *mut AnyObject {
  match with_mirror(None, |mirror| {
    mirror
      .marked
      .map(|(start, end)| (mirror.utf16_of(start), mirror.utf16_of(end)))
  }) {
    Some((start, end)) => range(start, end),
    None => std::ptr::null_mut(),
  }
}

extern "C-unwind" fn marked_text_style(_this: &AnyObject, _cmd: Sel) -> *mut AnyObject {
  std::ptr::null_mut()
}

extern "C-unwind" fn set_marked_text_style(_this: &AnyObject, _cmd: Sel, _style: *mut AnyObject) {}

extern "C-unwind" fn set_marked_text(_this: &AnyObject, _cmd: Sel, text: *mut AnyObject, selected: NSRange) {
  let text = string_of(text).unwrap_or_default();
  with_mirror((), |mirror| {
    let (start, end) = mirror.marked.unwrap_or(mirror.selection);
    mirror.replace(start, end, &text);
    // The selection UIKit gives is inside the marked text, in UTF-16 units.
    let inside = |utf16: usize| {
      let mut units = 0;
      for (byte, character) in text.char_indices() {
        if units >= utf16 {
          return byte;
        }
        units += character.len_utf16();
      }
      text.len()
    };
    let selection = (inside(selected.location), inside(selected.location + selected.length));
    if text.is_empty() {
      mirror.marked = None;
      mirror.selection = (start, start);
    } else {
      mirror.marked = Some((start, start + text.len()));
      mirror.selection = (start + selection.0, start + selection.1);
    }
    mirror.edits.push(Edit::Mark {
      selection: (!text.is_empty()).then_some(selection),
      text,
    });
  });
}

extern "C-unwind" fn unmark_text(_this: &AnyObject, _cmd: Sel) {
  with_mirror((), |mirror| {
    if let Some((start, end)) = mirror.marked.take() {
      let text = mirror.text[start..end].to_owned();
      mirror.edits.push(Edit::Unmark(text));
    }
  });
}

extern "C-unwind" fn beginning_of_document(_this: &AnyObject, _cmd: Sel) -> *mut AnyObject {
  position(0)
}

extern "C-unwind" fn end_of_document(_this: &AnyObject, _cmd: Sel) -> *mut AnyObject {
  position(with_mirror(0, |mirror| mirror.utf16_len()))
}

extern "C-unwind" fn text_range_from_position(
  _this: &AnyObject,
  _cmd: Sel,
  from: *mut AnyObject,
  to: *mut AnyObject,
) -> *mut AnyObject {
  match (offset_of(from), offset_of(to)) {
    (Some(from), Some(to)) => range(from, to),
    _ => std::ptr::null_mut(),
  }
}

extern "C-unwind" fn position_from_position(
  _this: &AnyObject,
  _cmd: Sel,
  from: *mut AnyObject,
  offset: isize,
) -> *mut AnyObject {
  let Some(from) = offset_of(from) else {
    return std::ptr::null_mut();
  };
  let length = with_mirror(0, |mirror| mirror.utf16_len());
  let to = from + offset;
  // No position before the start or beyond the end.
  if to < 0 || to > length {
    return std::ptr::null_mut();
  }
  position(to)
}

/// `UITextLayoutDirection`: right 2, left 3, up 4, down 5. Up and down go by lines as they are painted; the
/// text is read left to right.
extern "C-unwind" fn position_in_direction(
  this: &AnyObject,
  cmd: Sel,
  from: *mut AnyObject,
  direction: isize,
  offset: isize,
) -> *mut AnyObject {
  if matches!(direction, 4 | 5) {
    let Some(from) = offset_of(from) else {
      return std::ptr::null_mut();
    };
    let lines = if direction == 4 { -offset } else { offset };
    let to = with_mirror(None, |mirror| {
      let geometry = mirror.geometry.as_ref()?;
      let (x, y) = geometry.place(mirror.byte_of(from));
      let point = CGPoint {
        x: x - geometry.bounds.0,
        y: y - geometry.bounds.1 + geometry.line_height * (lines as f64 + 0.5),
      };
      Some(mirror.utf16_of(geometry.nearest(point)))
    });
    return match to {
      Some(to) => position(to),
      None => std::ptr::null_mut(),
    };
  }
  let offset = if direction == 2 { offset } else { -offset };
  position_from_position(this, cmd, from, offset)
}

/// `NSComparisonResult`.
extern "C-unwind" fn compare_position(
  _this: &AnyObject,
  _cmd: Sel,
  one: *mut AnyObject,
  other: *mut AnyObject,
) -> isize {
  match (offset_of(one), offset_of(other)) {
    (Some(one), Some(other)) => (one - other).signum(),
    _ => 0,
  }
}

extern "C-unwind" fn offset_from_position(
  _this: &AnyObject,
  _cmd: Sel,
  from: *mut AnyObject,
  to: *mut AnyObject,
) -> isize {
  match (offset_of(from), offset_of(to)) {
    (Some(from), Some(to)) => to - from,
    _ => 0,
  }
}

extern "C-unwind" fn input_delegate(_this: &AnyObject, _cmd: Sel) -> *mut AnyObject {
  INPUT_DELEGATE.load(Ordering::Acquire)
}

extern "C-unwind" fn set_input_delegate(_this: &AnyObject, _cmd: Sel, delegate: *mut AnyObject) {
  INPUT_DELEGATE.store(delegate, Ordering::Release);
}

extern "C-unwind" fn tokenizer(this: &AnyObject, _cmd: Sel) -> *mut AnyObject {
  let tokenizer = TOKENIZER.load(Ordering::Acquire);
  if !tokenizer.is_null() {
    return tokenizer;
  }
  // UIKit's own, which reads words and lines out of the text through this view.
  let tokenizer: Retained<AnyObject> = unsafe {
    let tokenizer: objc2::rc::Allocated<AnyObject> = msg_send![class!(UITextInputStringTokenizer), alloc];
    msg_send![tokenizer, initWithTextInput: this]
  };
  let tokenizer = Retained::into_raw(tokenizer);
  TOKENIZER.store(tokenizer, Ordering::Release);
  tokenizer
}

extern "C-unwind" fn position_within_range(
  _this: &AnyObject,
  _cmd: Sel,
  within: *mut AnyObject,
  direction: isize,
) -> *mut AnyObject {
  match ends_of(within) {
    Some((_, end)) if matches!(direction, 2 | 5) => position(end),
    Some((start, _)) => position(start),
    None => std::ptr::null_mut(),
  }
}

extern "C-unwind" fn character_range_by_extending(
  _this: &AnyObject,
  _cmd: Sel,
  from: *mut AnyObject,
  direction: isize,
) -> *mut AnyObject {
  let Some(from) = offset_of(from) else {
    return std::ptr::null_mut();
  };
  if matches!(direction, 2 | 5) {
    range(from, with_mirror(from, |mirror| mirror.utf16_len()))
  } else {
    range(0, from)
  }
}

/// `NSWritingDirection`: left to right.
extern "C-unwind" fn base_writing_direction(
  _this: &AnyObject,
  _cmd: Sel,
  _position: *mut AnyObject,
  _direction: isize,
) -> isize {
  0
}

extern "C-unwind" fn set_base_writing_direction(
  _this: &AnyObject,
  _cmd: Sel,
  _direction: isize,
  _range: *mut AnyObject,
) {
}

fn no_rect() -> CGRect {
  CGRect {
    origin: CGPoint { x: 0.0, y: 0.0 },
    size: CGSize {
      width: 0.0,
      height: 0.0,
    },
  }
}

/// The first line of the range: where UIKit puts what belongs to the range, such as the menu.
extern "C-unwind" fn first_rect_for_range(_this: &AnyObject, _cmd: Sel, range: *mut AnyObject) -> CGRect {
  let Some((start, end)) = ends_of(range) else {
    return no_rect();
  };
  with_mirror(None, |mirror| {
    let geometry = mirror.geometry.as_ref()?;
    let (left, row, right) = *geometry.rows(mirror.byte_of(start), mirror.byte_of(end)).first()?;
    Some(geometry.in_view(left, row, (right - left).max(CARET_WIDTH)))
  })
  .unwrap_or_else(no_rect)
}

/// The view the rectangles above are measured in, and the one the system puts its caret and selection into.
extern "C-unwind" fn text_input_view(this: &AnyObject, _cmd: Sel) -> *const AnyObject {
  this
}

extern "C-unwind" fn caret_rect_for_position(_this: &AnyObject, _cmd: Sel, position: *mut AnyObject) -> CGRect {
  let Some(offset) = offset_of(position) else {
    return no_rect();
  };
  with_mirror(None, |mirror| {
    let geometry = mirror.geometry.as_ref()?;
    let (x, y) = geometry.place(mirror.byte_of(offset));
    Some(geometry.in_view(x, y, CARET_WIDTH))
  })
  .unwrap_or_else(no_rect)
}

/// A `UITextSelectionRect`: one line of a selection, and whether the selection starts and ends on it.
fn selection_rect_class() -> &'static AnyClass {
  if let Some(class) = AnyClass::get(c"LurqTextSelectionRect") {
    return class;
  }
  fn part(this: &AnyObject, name: &std::ffi::CStr) -> f64 {
    let ivar = selection_rect_class()
      .instance_variable(name)
      .expect("an ivar the class was made with");
    unsafe { *ivar.load::<f64>(this) }
  }
  extern "C-unwind" fn rect(this: &AnyObject, _cmd: Sel) -> CGRect {
    CGRect {
      origin: CGPoint {
        x: part(this, c"lurqX"),
        y: part(this, c"lurqY"),
      },
      size: CGSize {
        width: part(this, c"lurqWidth"),
        height: part(this, c"lurqHeight"),
      },
    }
  }
  /// `NSWritingDirection`: left to right.
  extern "C-unwind" fn writing_direction(_this: &AnyObject, _cmd: Sel) -> isize {
    0
  }
  extern "C-unwind" fn contains_start(this: &AnyObject, _cmd: Sel) -> Bool {
    Bool::new(part(this, c"lurqStart") != 0.0)
  }
  extern "C-unwind" fn contains_end(this: &AnyObject, _cmd: Sel) -> Bool {
    Bool::new(part(this, c"lurqEnd") != 0.0)
  }
  extern "C-unwind" fn is_vertical(_this: &AnyObject, _cmd: Sel) -> Bool {
    Bool::NO
  }
  let mut builder =
    ClassBuilder::new(c"LurqTextSelectionRect", class!(UITextSelectionRect)).expect("a class name of its own");
  for name in [
    c"lurqX",
    c"lurqY",
    c"lurqWidth",
    c"lurqHeight",
    c"lurqStart",
    c"lurqEnd",
  ] {
    builder.add_ivar::<f64>(name);
  }
  unsafe {
    builder.add_method(sel!(rect), rect as extern "C-unwind" fn(_, _) -> _);
    builder.add_method(
      sel!(writingDirection),
      writing_direction as extern "C-unwind" fn(_, _) -> _,
    );
    builder.add_method(sel!(containsStart), contains_start as extern "C-unwind" fn(_, _) -> _);
    builder.add_method(sel!(containsEnd), contains_end as extern "C-unwind" fn(_, _) -> _);
    builder.add_method(sel!(isVertical), is_vertical as extern "C-unwind" fn(_, _) -> _);
  }
  builder.register()
}

extern "C-unwind" fn selection_rects_for_range(_this: &AnyObject, _cmd: Sel, range: *mut AnyObject) -> *mut AnyObject {
  // UIKit is about to show a selection, a handle's as it is dragged too: the shell reads where on its next
  // turn, which nothing else may bring, a handle's touches being UIKit's.
  let window_view = WINDOW_VIEW.load(Ordering::Acquire);
  if let Some(window_view) = unsafe { window_view.as_ref() } {
    let _: () = unsafe { msg_send![window_view, setNeedsDisplay] };
  }
  let rects: Retained<AnyObject> = unsafe { msg_send![class!(NSMutableArray), array] };
  let rows = ends_of(range).and_then(|(start, end)| {
    with_mirror(None, |mirror| {
      let geometry = mirror.geometry.as_ref()?;
      let rows = geometry.rows(mirror.byte_of(start), mirror.byte_of(end));
      Some(
        rows
          .into_iter()
          .map(|(left, row, right)| geometry.in_view(left, row, right - left))
          .collect::<Vec<_>>(),
      )
    })
  });
  let rows = rows.unwrap_or_default();
  let class = selection_rect_class();
  for (index, row) in rows.iter().enumerate() {
    let rect: Retained<AnyObject> = unsafe { msg_send![class, new] };
    let parts = [
      (c"lurqX", row.origin.x),
      (c"lurqY", row.origin.y),
      (c"lurqWidth", row.size.width),
      (c"lurqHeight", row.size.height),
      (c"lurqStart", f64::from(u8::from(index == 0))),
      (c"lurqEnd", f64::from(u8::from(index + 1 == rows.len()))),
    ];
    for (name, value) in parts {
      let ivar = class.instance_variable(name).expect("an ivar the class was made with");
      unsafe { *ivar.load_ptr::<f64>(&rect) = value };
    }
    let _: () = unsafe { msg_send![&*rects, addObject: &*rect] };
  }
  Retained::autorelease_ptr(rects)
}

/// The position nearest a point of the view, in UTF-16 units; the caret's when the input is not painted yet.
fn nearest_to(point: CGPoint) -> isize {
  with_mirror(0, |mirror| match &mirror.geometry {
    Some(geometry) => mirror.utf16_of(geometry.nearest(point)),
    None => mirror.utf16_of(mirror.selection.1),
  })
}

extern "C-unwind" fn closest_position_to_point(_this: &AnyObject, _cmd: Sel, point: CGPoint) -> *mut AnyObject {
  position(nearest_to(point))
}

extern "C-unwind" fn closest_position_within_range(
  _this: &AnyObject,
  _cmd: Sel,
  point: CGPoint,
  within: *mut AnyObject,
) -> *mut AnyObject {
  let nearest = nearest_to(point);
  match ends_of(within) {
    Some((start, end)) => position(nearest.clamp(start, end)),
    None => position(nearest),
  }
}

extern "C-unwind" fn character_range_at_point(_this: &AnyObject, _cmd: Sel, point: CGPoint) -> *mut AnyObject {
  let nearest = nearest_to(point);
  let length = with_mirror(0, |mirror| mirror.utf16_len());
  if nearest >= length {
    return std::ptr::null_mut();
  }
  range(nearest, nearest + 1)
}

// The menu over a selection: UIKit asks what can be done and tells the view to do it.

/// Tells UIKit the selection changed under it: the menu's actions change the copy from inside UIKit's call.
fn selection_changed_by(this: &AnyObject, change: impl FnOnce()) {
  let delegate = INPUT_DELEGATE.load(Ordering::Acquire);
  if let Some(delegate) = unsafe { delegate.as_ref() } {
    let _: () = unsafe { msg_send![delegate, selectionWillChange: this] };
  }
  change();
  if let Some(delegate) = unsafe { delegate.as_ref() } {
    let _: () = unsafe { msg_send![delegate, selectionDidChange: this] };
  }
}

fn selected_text() -> Option<String> {
  with_mirror(None, |mirror| {
    let (start, end) = mirror.selection;
    // A password is not copied out of its input.
    let secure = mirror.traits.is_some_and(|traits| traits.secure);
    (start != end && !secure).then(|| mirror.text[start..end].to_owned())
  })
}

fn pasteboard() -> Retained<AnyObject> {
  unsafe { msg_send![class!(UIPasteboard), generalPasteboard] }
}

extern "C-unwind" fn copy(_this: &AnyObject, _cmd: Sel, _sender: *mut AnyObject) {
  if let Some(text) = selected_text() {
    let _: () = unsafe { msg_send![&*pasteboard(), setString: ns_string(&text)] };
  }
}

extern "C-unwind" fn cut(this: &AnyObject, cmd: Sel, sender: *mut AnyObject) {
  if selected_text().is_none() {
    return;
  }
  copy(this, cmd, sender);
  delete_backward(this, cmd);
}

extern "C-unwind" fn paste(this: &AnyObject, cmd: Sel, _sender: *mut AnyObject) {
  let text: Option<Retained<AnyObject>> = unsafe { msg_send![&*pasteboard(), string] };
  if let Some(text) = text {
    insert_text(this, cmd, Retained::as_ptr(&text).cast_mut());
  }
}

extern "C-unwind" fn select_all(this: &AnyObject, _cmd: Sel, _sender: *mut AnyObject) {
  selection_changed_by(this, || {
    with_mirror((), |mirror| {
      mirror.selection = (0, mirror.text.len());
      mirror.edits.push(Edit::Select {
        start: 0,
        end: mirror.text.len(),
      });
    });
  });
}

extern "C-unwind" fn can_perform_action(_this: &AnyObject, _cmd: Sel, action: Sel, _sender: *mut AnyObject) -> Bool {
  let selected = selected_text().is_some();
  let allowed = if action == sel!(copy:) || action == sel!(cut:) {
    selected
  } else if action == sel!(paste:) {
    unsafe { msg_send![&*pasteboard(), hasStrings] }
  } else if action == sel!(selectAll:) {
    with_mirror(false, |mirror| {
      !mirror.text.is_empty() && mirror.selection != (0, mirror.text.len())
    })
  } else {
    false
  };
  Bool::new(allowed)
}

// The keyboard's traits: `UITextInputTraits`.

fn traits() -> Option<KeyboardTraits> {
  with_mirror(None, |mirror| mirror.traits)
}

extern "C-unwind" fn is_secure_text_entry(_this: &AnyObject, _cmd: Sel) -> Bool {
  Bool::new(traits().is_some_and(|traits| traits.secure))
}

/// `UIKeyboardType`.
extern "C-unwind" fn keyboard_type(_this: &AnyObject, _cmd: Sel) -> isize {
  match traits().map(|traits| traits.kind) {
    Some(KeyboardKind::Url) => 3,
    Some(KeyboardKind::Number) => 4,
    Some(KeyboardKind::Phone) => 5,
    Some(KeyboardKind::Email) => 7,
    Some(KeyboardKind::Decimal) => 8,
    Some(KeyboardKind::Search) => 10,
    Some(KeyboardKind::Text) | None => 0,
  }
}

/// `UIReturnKeyType`.
extern "C-unwind" fn return_key_type(_this: &AnyObject, _cmd: Sel) -> isize {
  match traits().map(|traits| traits.return_key) {
    Some(ReturnKey::Go) => 1,
    Some(ReturnKey::Next) => 4,
    Some(ReturnKey::Search) => 6,
    Some(ReturnKey::Send) => 7,
    Some(ReturnKey::Done) => 9,
    Some(ReturnKey::Default) | None => 0,
  }
}

#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {
  static UITextContentTypeName: *mut AnyObject;
  static UITextContentTypeEmailAddress: *mut AnyObject;
  static UITextContentTypeUsername: *mut AnyObject;
  static UITextContentTypePassword: *mut AnyObject;
  static UITextContentTypeNewPassword: *mut AnyObject;
  static UITextContentTypeOneTimeCode: *mut AnyObject;
  static UITextContentTypeTelephoneNumber: *mut AnyObject;
  static UITextContentTypeURL: *mut AnyObject;
}

/// `UITextContentType`: what the system offers to fill the input in with.
extern "C-unwind" fn text_content_type(_this: &AnyObject, _cmd: Sel) -> *mut AnyObject {
  unsafe {
    match traits().map(|traits| traits.content) {
      Some(ContentKind::Name) => UITextContentTypeName,
      Some(ContentKind::Email) => UITextContentTypeEmailAddress,
      Some(ContentKind::Username) => UITextContentTypeUsername,
      Some(ContentKind::Password) => UITextContentTypePassword,
      Some(ContentKind::NewPassword) => UITextContentTypeNewPassword,
      Some(ContentKind::OneTimeCode) => UITextContentTypeOneTimeCode,
      Some(ContentKind::Phone) => UITextContentTypeTelephoneNumber,
      Some(ContentKind::Url) => UITextContentTypeURL,
      Some(ContentKind::Auto | ContentKind::None) | None => std::ptr::null_mut(),
    }
  }
}

/// Whether the input holds prose: what is not a password, an address or a number is corrected and
/// capitalised as prose is.
fn is_prose() -> bool {
  traits().is_none_or(|traits| !traits.secure && matches!(traits.kind, KeyboardKind::Text | KeyboardKind::Search))
}

/// `UITextAutocapitalizationType`: sentences, or none.
extern "C-unwind" fn autocapitalization_type(_this: &AnyObject, _cmd: Sel) -> isize {
  if is_prose() { 2 } else { 0 }
}

/// `UITextAutocorrectionType` and `UITextSpellCheckingType`: the default, or no.
extern "C-unwind" fn correction_type(_this: &AnyObject, _cmd: Sel) -> isize {
  if is_prose() { 0 } else { 1 }
}

/// The class of the view that takes the keyboard, registered once.
fn text_input_class() -> &'static AnyClass {
  if let Some(class) = AnyClass::get(c"LurqTextInputView") {
    return class;
  }
  let mut builder = ClassBuilder::new(c"LurqTextInputView", class!(UIView)).expect("a class name of its own");
  for protocol in [c"UIKeyInput", c"UITextInputTraits", c"UITextInput"] {
    if let Some(protocol) = AnyProtocol::get(protocol) {
      builder.add_protocol(protocol);
    }
  }
  macro_rules! methods {
    ($($selector:expr => $function:expr),* $(,)?) => {
      unsafe {
        $(builder.add_method($selector, $function);)*
      }
    };
  }
  methods! {
    sel!(canBecomeFirstResponder) => can_become_first_responder as extern "C-unwind" fn(_, _) -> _,
    sel!(hasText) => has_text as extern "C-unwind" fn(_, _) -> _,
    sel!(insertText:) => insert_text as extern "C-unwind" fn(_, _, _),
    sel!(deleteBackward) => delete_backward as extern "C-unwind" fn(_, _),
    sel!(textInRange:) => text_in_range as extern "C-unwind" fn(_, _, _) -> _,
    sel!(replaceRange:withText:) => replace_range as extern "C-unwind" fn(_, _, _, _),
    sel!(selectedTextRange) => selected_text_range as extern "C-unwind" fn(_, _) -> _,
    sel!(setSelectedTextRange:) => set_selected_text_range as extern "C-unwind" fn(_, _, _),
    sel!(markedTextRange) => marked_text_range as extern "C-unwind" fn(_, _) -> _,
    sel!(markedTextStyle) => marked_text_style as extern "C-unwind" fn(_, _) -> _,
    sel!(setMarkedTextStyle:) => set_marked_text_style as extern "C-unwind" fn(_, _, _),
    sel!(setMarkedText:selectedRange:) => set_marked_text as extern "C-unwind" fn(_, _, _, _),
    sel!(unmarkText) => unmark_text as extern "C-unwind" fn(_, _),
    sel!(beginningOfDocument) => beginning_of_document as extern "C-unwind" fn(_, _) -> _,
    sel!(endOfDocument) => end_of_document as extern "C-unwind" fn(_, _) -> _,
    sel!(textRangeFromPosition:toPosition:) => text_range_from_position as extern "C-unwind" fn(_, _, _, _) -> _,
    sel!(positionFromPosition:offset:) => position_from_position as extern "C-unwind" fn(_, _, _, _) -> _,
    sel!(positionFromPosition:inDirection:offset:) =>
      position_in_direction as extern "C-unwind" fn(_, _, _, _, _) -> _,
    sel!(comparePosition:toPosition:) => compare_position as extern "C-unwind" fn(_, _, _, _) -> _,
    sel!(offsetFromPosition:toPosition:) => offset_from_position as extern "C-unwind" fn(_, _, _, _) -> _,
    sel!(inputDelegate) => input_delegate as extern "C-unwind" fn(_, _) -> _,
    sel!(setInputDelegate:) => set_input_delegate as extern "C-unwind" fn(_, _, _),
    sel!(tokenizer) => tokenizer as extern "C-unwind" fn(_, _) -> _,
    sel!(positionWithinRange:farthestInDirection:) =>
      position_within_range as extern "C-unwind" fn(_, _, _, _) -> _,
    sel!(characterRangeByExtendingPosition:inDirection:) =>
      character_range_by_extending as extern "C-unwind" fn(_, _, _, _) -> _,
    sel!(baseWritingDirectionForPosition:inDirection:) =>
      base_writing_direction as extern "C-unwind" fn(_, _, _, _) -> _,
    sel!(setBaseWritingDirection:forRange:) => set_base_writing_direction as extern "C-unwind" fn(_, _, _, _),
    sel!(textInputView) => text_input_view as extern "C-unwind" fn(_, _) -> _,
    sel!(firstRectForRange:) => first_rect_for_range as extern "C-unwind" fn(_, _, _) -> _,
    sel!(caretRectForPosition:) => caret_rect_for_position as extern "C-unwind" fn(_, _, _) -> _,
    sel!(selectionRectsForRange:) => selection_rects_for_range as extern "C-unwind" fn(_, _, _) -> _,
    sel!(closestPositionToPoint:) => closest_position_to_point as extern "C-unwind" fn(_, _, _) -> _,
    sel!(closestPositionToPoint:withinRange:) =>
      closest_position_within_range as extern "C-unwind" fn(_, _, _, _) -> _,
    sel!(characterRangeAtPoint:) => character_range_at_point as extern "C-unwind" fn(_, _, _) -> _,
    sel!(copy:) => copy as extern "C-unwind" fn(_, _, _),
    sel!(cut:) => cut as extern "C-unwind" fn(_, _, _),
    sel!(paste:) => paste as extern "C-unwind" fn(_, _, _),
    sel!(selectAll:) => select_all as extern "C-unwind" fn(_, _, _),
    sel!(canPerformAction:withSender:) => can_perform_action as extern "C-unwind" fn(_, _, _, _) -> _,
    sel!(isSecureTextEntry) => is_secure_text_entry as extern "C-unwind" fn(_, _) -> _,
    sel!(keyboardType) => keyboard_type as extern "C-unwind" fn(_, _) -> _,
    sel!(returnKeyType) => return_key_type as extern "C-unwind" fn(_, _) -> _,
    sel!(autocapitalizationType) => autocapitalization_type as extern "C-unwind" fn(_, _) -> _,
    sel!(autocorrectionType) => correction_type as extern "C-unwind" fn(_, _) -> _,
    sel!(spellCheckingType) => correction_type as extern "C-unwind" fn(_, _) -> _,
    sel!(textContentType) => text_content_type as extern "C-unwind" fn(_, _) -> _,
  }
  builder.register()
}

/// What UIKit is told changed when the tree's input is not what the copy says.
enum Changed {
  Text,
  Selection,
  /// Neither: the same text and selection are painted elsewhere. UIKit's delegate is not told, which costs
  /// tens of milliseconds for it to read the text anew; only the caret and the selection are drawn anew.
  Place,
}

/// The view the keyboard types into. Main thread only.
pub(super) struct TextInput {
  view: Retained<AnyObject>,
}

impl TextInput {
  /// Adds the view, which shows nothing, to the window's view.
  pub(super) fn new(parent: NonNull<c_void>) -> Self {
    let parent: &AnyObject = unsafe { parent.cast::<AnyObject>().as_ref() };
    WINDOW_VIEW.store(std::ptr::from_ref(parent).cast_mut(), Ordering::Release);
    unsafe {
      let view: Retained<AnyObject> = msg_send![text_input_class(), new];
      // No size until an input has the focus, so nothing to see or touch; a hidden view could not take the
      // keyboard.
      let _: () = msg_send![&*view, setUserInteractionEnabled: false];
      let _: () = msg_send![parent, addSubview: &*view];
      // The platform's caret, selection handles, loupe and menu: `UITextInteractionModeEditable`.
      let interaction: Retained<AnyObject> = msg_send![class!(UITextInteraction), textInteractionForMode: 0isize];
      let _: () = msg_send![&*interaction, setTextInput: &*view];
      let _: () = msg_send![&*view, addInteraction: &*interaction];
      Self { view }
    }
  }

  /// One turn of the shell: makes UIKit's edits in the tree, then brings the keyboard and the copy to what the
  /// tree's focused text input now is.
  pub(super) fn sync(&mut self, tree: &mut Tree) {
    let edits = with_mirror(Vec::new(), |mirror| std::mem::take(&mut mirror.edits));
    for edit in edits {
      match edit {
        Edit::Insert(text) => tree.keyboard_insert(&text),
        Edit::DeleteBackward => tree.keyboard_delete_backward(),
        Edit::Replace { start, end, text } => tree.text_input_replace(start..end, &text),
        Edit::Select { start, end } => tree.text_input_select(start..end),
        Edit::Mark { text, selection } => tree.ime(ImeEvent::Preedit {
          text,
          cursor: selection,
        }),
        Edit::Unmark(text) => tree.ime(ImeEvent::Commit(text)),
      }
    }
    let shown = self.shown_selection();
    let changed = {
      let mut highlight = HIGHLIGHT.lock().unwrap();
      let changed = *highlight != shown;
      if changed {
        highlight.clone_from(&shown);
      }
      changed
    };
    if changed {
      tree.set_native_selection_rects(
        shown
          .into_iter()
          .map(|(x, y, width, height)| (x as f32, y as f32, width as f32, height as f32))
          .collect(),
      );
    }
    let traits = tree.keyboard_traits();
    let snapshot = tree.text_input_snapshot();
    let (Some(traits), Some(snapshot)) = (traits, snapshot) else {
      self.dismiss();
      return;
    };
    let previous = with_mirror(None, |mirror| mirror.traits);
    let geometry = snapshot.geometry.as_ref().map(|geometry| Geometry {
      bounds: (
        f64::from(geometry.bounds.0),
        f64::from(geometry.bounds.1),
        f64::from(geometry.bounds.2),
        f64::from(geometry.bounds.3),
      ),
      carets: geometry
        .carets
        .iter()
        .map(|(index, x, y)| (*index, f64::from(*x), f64::from(*y)))
        .collect(),
      line_height: f64::from(geometry.line_height),
    });
    let moved = with_mirror(true, |mirror| mirror.geometry != geometry);
    // Text that is the same but painted elsewhere, scrolled or laid out anew, is a caret to draw anew.
    let changed = Self::changed(&snapshot).or(moved.then_some(Changed::Place));
    if let Some(changed) = &changed {
      self.tell(changed, true);
    }
    {
      let mut mirror = MIRROR.lock().unwrap();
      let mirror = mirror.get_or_insert_with(Mirror::default);
      mirror.traits = Some(traits);
      mirror.geometry = geometry.clone();
      if matches!(changed, Some(Changed::Text | Changed::Selection)) {
        mirror.text = snapshot.text;
        mirror.selection = (snapshot.selection.start, snapshot.selection.end);
        mirror.marked = snapshot.composition.map(|marked| (marked.start, marked.end));
      }
    }
    // The view lies over the input, for the platform's caret and handles to be drawn and touched there.
    if moved && let Some(geometry) = &geometry {
      let frame = CGRect {
        origin: CGPoint {
          x: geometry.bounds.0,
          y: geometry.bounds.1,
        },
        size: CGSize {
          width: geometry.bounds.2,
          height: geometry.bounds.3,
        },
      };
      unsafe {
        let _: () = msg_send![&*self.view, setFrame: frame];
        let _: () = msg_send![&*self.view, setUserInteractionEnabled: true];
      }
    }
    if let Some(changed) = &changed {
      self.tell(changed, false);
      self.update_selection();
    }
    unsafe {
      match previous {
        None => {
          let _: () = msg_send![&*self.view, setHidden: false];
          let _: bool = msg_send![&*self.view, becomeFirstResponder];
          self.show_selection(true);
        }
        // Another input took the focus: the keyboard stays up and becomes the one it asks for.
        Some(previous) if previous != traits => {
          let _: () = msg_send![&*self.view, reloadInputViews];
        }
        Some(_) => {}
      }
    }
  }

  /// Has the platform draw its caret and selection, or stop. The text interaction makes the interaction that
  /// draws them and leaves it off for a view that is not one of UIKit's own.
  fn show_selection(&self, shown: bool) {
    if shown {
      self.lower_surface();
    }
    if let Some(display) = self.selection_display() {
      unsafe {
        let _: () = msg_send![&*display, setActivated: shown];
        // The input paints the selection's highlight itself, under its text, where the platform's view says
        // it is (`shown_selection`); that view would lie over the text, and is not seen. It stays UIKit's
        // own: with a view of the shell's in its place UIKit showed no menu.
        let highlight: Option<Retained<AnyObject>> = msg_send![&*display, highlightView];
        if let Some(highlight) = highlight {
          let _: () = msg_send![&*highlight, setAlpha: 0.0f64];
        }
        let _: () = msg_send![&*display, setNeedsSelectionUpdate];
      }
    }
  }

  /// The rectangles UIKit shows the selection as, in the window: the selection's, and a handle's as it is
  /// dragged, when the selection itself is not yet set.
  fn shown_selection(&self) -> Vec<(f64, f64, f64, f64)> {
    let Some(display) = self.selection_display() else {
      return Vec::new();
    };
    unsafe {
      let highlight: Option<Retained<AnyObject>> = msg_send![&*display, highlightView];
      let Some(highlight) = highlight else {
        return Vec::new();
      };
      let rects: Option<Retained<AnyObject>> = msg_send![&*highlight, selectionRects];
      let Some(rects) = rects else {
        return Vec::new();
      };
      let count: usize = msg_send![&*rects, count];
      (0..count)
        .map(|index| {
          let rect: Retained<AnyObject> = msg_send![&*rects, objectAtIndex: index];
          let rect: CGRect = msg_send![&*rect, rect];
          let rect: CGRect = msg_send![&*highlight, convertRect: rect, toView: std::ptr::null::<AnyObject>()];
          (rect.origin.x, rect.origin.y, rect.size.width, rect.size.height)
        })
        .collect()
    }
  }

  /// Puts the layer lurq renders into under the views of the window's view. wgpu adds it as a sublayer after
  /// them, which is over them: nothing a view draws, the caret and the selection here, would be seen.
  fn lower_surface(&self) {
    unsafe {
      let parent: Option<Retained<AnyObject>> = msg_send![&*self.view, superview];
      let Some(parent) = parent else {
        return;
      };
      let layer: Retained<AnyObject> = msg_send![&*parent, layer];
      let sublayers: Option<Retained<AnyObject>> = msg_send![&*layer, sublayers];
      let Some(sublayers) = sublayers else {
        return;
      };
      let count: usize = msg_send![&*sublayers, count];
      for index in 0..count {
        let sublayer: Retained<AnyObject> = msg_send![&*sublayers, objectAtIndex: index];
        let renders: bool = msg_send![&*sublayer, isKindOfClass: class!(CAMetalLayer)];
        if renders {
          let _: () = msg_send![&*sublayer, setZPosition: -1.0f64];
        }
      }
    }
  }

  /// Has the platform draw the caret and selection anew, where the view now says they are.
  fn update_selection(&self) {
    if let Some(display) = self.selection_display() {
      let _: () = unsafe { msg_send![&*display, setNeedsSelectionUpdate] };
      // Now, for the caret to move in the frame its character is drawn in and not a frame later.
      let _: () = unsafe { msg_send![&*display, layoutManagedSubviews] };
    }
  }

  /// The interaction of the view that draws the caret and the selection.
  fn selection_display(&self) -> Option<Retained<AnyObject>> {
    unsafe {
      let interactions: Retained<AnyObject> = msg_send![&*self.view, interactions];
      let count: usize = msg_send![&*interactions, count];
      (0..count)
        .map(|index| -> Retained<AnyObject> { msg_send![&*interactions, objectAtIndex: index] })
        .find(|interaction| msg_send![&**interaction, isKindOfClass: class!(UITextSelectionDisplayInteraction)])
    }
  }

  /// How the tree's input differs from the copy, if it does.
  fn changed(snapshot: &TextInputSnapshot) -> Option<Changed> {
    let mirror = MIRROR.lock().unwrap();
    let Some(mirror) = mirror.as_ref() else {
      return Some(Changed::Text);
    };
    if mirror.text != snapshot.text {
      Some(Changed::Text)
    } else if mirror.selection != (snapshot.selection.start, snapshot.selection.end)
      || mirror.marked != snapshot.composition.as_ref().map(|marked| (marked.start, marked.end))
    {
      Some(Changed::Selection)
    } else {
      None
    }
  }

  /// Tells UIKit the text or the selection is about to change, or has. It asks the view anew, which answers
  /// from the copy: the copy's lock is not held here.
  fn tell(&self, changed: &Changed, before: bool) {
    let delegate = INPUT_DELEGATE.load(Ordering::Acquire);
    let Some(delegate) = (unsafe { delegate.as_ref() }) else {
      return;
    };
    unsafe {
      let _: () = match (changed, before) {
        (Changed::Text, true) => msg_send![delegate, textWillChange: &*self.view],
        (Changed::Text, false) => msg_send![delegate, textDidChange: &*self.view],
        (Changed::Selection, true) => msg_send![delegate, selectionWillChange: &*self.view],
        (Changed::Selection, false) => msg_send![delegate, selectionDidChange: &*self.view],
        (Changed::Place, _) => return,
      };
    }
  }

  /// No text input has focus: the keyboard goes away.
  fn dismiss(&mut self) {
    if MIRROR.lock().unwrap().take().is_none() {
      return;
    }
    unsafe {
      self.show_selection(false);
      let _: () = msg_send![&*self.view, setUserInteractionEnabled: false];
      // Hidden where it is: moved away, its caret was at times seen on its way out, at the window's corner.
      let _: () = msg_send![&*self.view, setHidden: true];
      let _: bool = msg_send![&*self.view, resignFirstResponder];
      // When this view gives the keyboard up UIKit hands it to the next view that takes keys, which is
      // winit's, and the keyboard stays. Whoever holds it now lets it go.
      let app: Retained<AnyObject> = msg_send![class!(UIApplication), sharedApplication];
      let _: bool = msg_send![
        &*app,
        sendAction: sel!(resignFirstResponder),
        to: std::ptr::null::<AnyObject>(),
        from: std::ptr::null::<AnyObject>(),
        forEvent: std::ptr::null::<AnyObject>()
      ];
    }
  }
}
