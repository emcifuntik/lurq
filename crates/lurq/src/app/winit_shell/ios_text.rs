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
//! Where characters are on the screen is answered with the caret's rectangle, whatever position is asked
//! about: the tree does not tell the shell more yet.

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
  /// The caret, in points in the window.
  caret: Option<(f64, f64, f64, f64)>,
  traits: Option<KeyboardTraits>,
  edits: Vec<Edit>,
}

static MIRROR: Mutex<Option<Mirror>> = Mutex::new(None);
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

/// `UITextLayoutDirection`: right 2, left 3, up 4, down 5. The text is taken as one line read left to right.
extern "C-unwind" fn position_in_direction(
  this: &AnyObject,
  cmd: Sel,
  from: *mut AnyObject,
  direction: isize,
  offset: isize,
) -> *mut AnyObject {
  let offset = match direction {
    2 | 5 => offset,
    _ => -offset,
  };
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

/// The caret, for whatever place is asked about; see the module's last paragraph.
fn caret_rect() -> CGRect {
  let (x, y, width, height) = with_mirror(None, |mirror| mirror.caret).unwrap_or((0.0, 0.0, 0.0, 0.0));
  CGRect {
    origin: CGPoint { x, y },
    size: CGSize { width, height },
  }
}

extern "C-unwind" fn first_rect_for_range(_this: &AnyObject, _cmd: Sel, _range: *mut AnyObject) -> CGRect {
  caret_rect()
}

extern "C-unwind" fn caret_rect_for_position(_this: &AnyObject, _cmd: Sel, _position: *mut AnyObject) -> CGRect {
  caret_rect()
}

extern "C-unwind" fn selection_rects_for_range(_this: &AnyObject, _cmd: Sel, _range: *mut AnyObject) -> *mut AnyObject {
  let rects: Retained<AnyObject> = unsafe { msg_send![class!(NSArray), array] };
  Retained::autorelease_ptr(rects)
}

extern "C-unwind" fn closest_position_to_point(_this: &AnyObject, _cmd: Sel, _point: CGPoint) -> *mut AnyObject {
  position(with_mirror(0, |mirror| mirror.utf16_of(mirror.selection.1)))
}

extern "C-unwind" fn closest_position_within_range(
  _this: &AnyObject,
  _cmd: Sel,
  _point: CGPoint,
  within: *mut AnyObject,
) -> *mut AnyObject {
  let caret = with_mirror(0, |mirror| mirror.utf16_of(mirror.selection.1));
  match ends_of(within) {
    Some((start, end)) => position(caret.clamp(start, end)),
    None => position(caret),
  }
}

extern "C-unwind" fn character_range_at_point(_this: &AnyObject, _cmd: Sel, _point: CGPoint) -> *mut AnyObject {
  std::ptr::null_mut()
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
    sel!(firstRectForRange:) => first_rect_for_range as extern "C-unwind" fn(_, _, _) -> _,
    sel!(caretRectForPosition:) => caret_rect_for_position as extern "C-unwind" fn(_, _, _) -> _,
    sel!(selectionRectsForRange:) => selection_rects_for_range as extern "C-unwind" fn(_, _, _) -> _,
    sel!(closestPositionToPoint:) => closest_position_to_point as extern "C-unwind" fn(_, _, _) -> _,
    sel!(closestPositionToPoint:withinRange:) =>
      closest_position_within_range as extern "C-unwind" fn(_, _, _, _) -> _,
    sel!(characterRangeAtPoint:) => character_range_at_point as extern "C-unwind" fn(_, _, _) -> _,
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
}

/// The view the keyboard types into. Main thread only.
pub(super) struct TextInput {
  view: Retained<AnyObject>,
}

impl TextInput {
  /// Adds the view, which shows nothing, to the window's view.
  pub(super) fn new(parent: NonNull<c_void>) -> Self {
    let parent: &AnyObject = unsafe { parent.cast::<AnyObject>().as_ref() };
    unsafe {
      let view: Retained<AnyObject> = msg_send![text_input_class(), new];
      // No size, so nothing to see or touch; a hidden view could not take the keyboard.
      let _: () = msg_send![&*view, setUserInteractionEnabled: false];
      let _: () = msg_send![parent, addSubview: &*view];
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

    let traits = tree.keyboard_traits();
    let snapshot = tree.text_input_snapshot();
    let (Some(traits), Some(snapshot)) = (traits, snapshot) else {
      self.dismiss();
      return;
    };
    let previous = with_mirror(None, |mirror| mirror.traits);
    let changed = Self::changed(&snapshot);
    if let Some(changed) = &changed {
      self.tell(changed, true);
    }
    {
      let mut mirror = MIRROR.lock().unwrap();
      let mirror = mirror.get_or_insert_with(Mirror::default);
      mirror.traits = Some(traits);
      mirror.caret = snapshot.caret.map(|caret| {
        (
          f64::from(caret.x),
          f64::from(caret.y),
          f64::from(caret.width),
          f64::from(caret.height),
        )
      });
      if changed.is_some() {
        mirror.text = snapshot.text;
        mirror.selection = (snapshot.selection.start, snapshot.selection.end);
        mirror.marked = snapshot.composition.map(|marked| (marked.start, marked.end));
      }
    }
    if let Some(changed) = &changed {
      self.tell(changed, false);
    }
    unsafe {
      match previous {
        None => {
          let _: bool = msg_send![&*self.view, becomeFirstResponder];
        }
        // Another input took the focus: the keyboard stays up and becomes the one it asks for.
        Some(previous) if previous != traits => {
          let _: () = msg_send![&*self.view, reloadInputViews];
        }
        Some(_) => {}
      }
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
      };
    }
  }

  /// No text input has focus: the keyboard goes away.
  fn dismiss(&mut self) {
    if MIRROR.lock().unwrap().take().is_none() {
      return;
    }
    unsafe {
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
