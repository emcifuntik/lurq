#[cfg(any(feature = "persistent_storage", feature = "resources"))]
use std::path::PathBuf;
use std::{path::Path, sync::Arc};

use parking_lot::Mutex;

#[cfg(feature = "i18n")]
use crate::app::i18n::I18n;
use crate::app::{glyph_engine::GlyphEngine, theme::Theme};

/// A request to open a new OS window that renders its own component tree.
/// Created through [`WindowOpener::open`], drained by the runtime each loop
/// turn (the winit shell then creates the OS window and renders the tree with
/// the same render-engine factory as the main window).
pub struct SecondaryWindowRequest {
  pub(crate) title: String,
  pub(crate) width: u32,
  pub(crate) height: u32,
  /// `false` creates the OS window without native decorations, for trees that
  /// render their own `WindowChrome`.
  pub(crate) decorations: bool,
  /// Optional stable machine name (titles aren't unique) surfaced to external
  /// tooling such as the MCP `lurq_windows` tool.
  pub(crate) name: Option<String>,
  pub(crate) build: Box<dyn FnOnce(&mut App, &mut crate::app::Tree) + Send>,
}

/// Options for [`WindowOpener::open_with`] — the builder-style variant of
/// [`WindowOpener::open`] for windows that need a name or no decorations.
pub struct WindowOptions {
  title: String,
  width: u32,
  height: u32,
  decorations: bool,
  name: Option<String>,
}

impl WindowOptions {
  pub fn new(title: impl Into<String>, width: u32, height: u32) -> Self {
    Self {
      title: title.into(),
      width,
      height,
      decorations: true,
      name: None,
    }
  }

  /// Create the OS window without native decorations (for trees that render
  /// their own `WindowChrome`).
  pub fn undecorated(mut self) -> Self {
    self.decorations = false;
    self
  }

  /// Stable machine name for external tooling to target this window by.
  pub fn window_name(mut self, name: impl Into<String>) -> Self {
    self.name = Some(name.into());
    self
  }
}

/// Cloneable handle for opening secondary windows from anywhere — including
/// event handlers, which don't have `Ctx` access. Obtain it via
/// `ctx.window_opener()` (or [`App::window_opener`]).
#[derive(Clone, Default)]
pub struct WindowOpener {
  queue: std::sync::Arc<std::sync::Mutex<Vec<SecondaryWindowRequest>>>,
}

impl WindowOpener {
  /// Queue a new secondary window with the given title and logical size.
  /// `build` mounts the window's root component into a fresh [`crate::app::Tree`]:
  ///
  /// ```ignore
  /// opener.open("Preview", 1100, 800, move |app, tree| {
  ///   tree.mount_root::<PreviewWindow>(app, props);
  /// });
  /// ```
  pub fn open<F>(&self, title: impl Into<String>, width: u32, height: u32, build: F)
  where
    F: FnOnce(&mut App, &mut crate::app::Tree) + Send + 'static,
  {
    self.request(title, width, height, true, build);
  }

  /// Like [`open`](Self::open), but the OS window is created without native
  /// decorations — for windows that render their own `WindowChrome`.
  pub fn open_undecorated<F>(&self, title: impl Into<String>, width: u32, height: u32, build: F)
  where
    F: FnOnce(&mut App, &mut crate::app::Tree) + Send + 'static,
  {
    self.request(title, width, height, false, build);
  }

  /// Queue a secondary window described by [`WindowOptions`], for windows
  /// that need a stable name or no decorations.
  pub fn open_with<F>(&self, options: WindowOptions, build: F)
  where
    F: FnOnce(&mut App, &mut crate::app::Tree) + Send + 'static,
  {
    self.queue.lock().unwrap().push(SecondaryWindowRequest {
      title: options.title,
      width: options.width,
      height: options.height,
      decorations: options.decorations,
      name: options.name,
      build: Box::new(build),
    });
  }

  fn request<F>(&self, title: impl Into<String>, width: u32, height: u32, decorations: bool, build: F)
  where
    F: FnOnce(&mut App, &mut crate::app::Tree) + Send + 'static,
  {
    self.queue.lock().unwrap().push(SecondaryWindowRequest {
      title: title.into(),
      width,
      height,
      decorations,
      name: None,
      build: Box::new(build),
    });
  }

  pub(crate) fn take(&self) -> Vec<SecondaryWindowRequest> {
    std::mem::take(&mut self.queue.lock().unwrap())
  }
}

/// A handle to shared application services. Moving or cloning it does not move
/// the state used by mounted components; trees retain that state while needed.
#[derive(Clone)]
pub struct App {
  pub(crate) shared: Arc<AppShared>,
}

pub(crate) struct AppShared {
  pub(crate) menu: super::menu::MenuController,
  pub(crate) glyph_engine: Mutex<GlyphEngine>,
  pub(crate) theme: Theme,
  pub(crate) window_opener: WindowOpener,
  #[cfg(feature = "i18n")]
  pub(crate) i18n: I18n,
  pub(crate) scale_override: Mutex<Option<f32>>,
  #[cfg(feature = "tokio")]
  pub(crate) tokio_handle: Mutex<Option<tokio::runtime::Handle>>,
  #[cfg(feature = "resources")]
  pub(crate) resource_loader: Mutex<crate::resources::ResourceLoader>,
  #[cfg(feature = "persistent_storage")]
  pub(crate) persistent_storage: Mutex<crate::persistent_storage::PersistentStorage>,
  #[cfg(all(feature = "image", feature = "resources"))]
  pub(crate) image_resource_cache: Mutex<std::collections::HashMap<std::sync::Arc<str>, crate::images::ImageData>>,
  #[cfg(all(feature = "svg", feature = "resources"))]
  pub(crate) svg_resource_cache: Mutex<std::collections::HashMap<std::sync::Arc<str>, crate::svg::SvgData>>,
}

impl Default for App {
  fn default() -> Self {
    Self::new()
  }
}

impl App {
  pub fn new() -> Self {
    Self {
      shared: Arc::new(AppShared {
        menu: super::menu::MenuController::default(),
        glyph_engine: Mutex::new(GlyphEngine::new()),
        theme: Theme::new(),
        window_opener: WindowOpener::default(),
        #[cfg(feature = "i18n")]
        i18n: I18n::new(),
        scale_override: Mutex::new(None),
        #[cfg(feature = "tokio")]
        tokio_handle: Mutex::new(None),
        #[cfg(feature = "resources")]
        resource_loader: Mutex::new(crate::resources::ResourceLoader::new()),
        #[cfg(feature = "persistent_storage")]
        persistent_storage: Mutex::new(crate::persistent_storage::PersistentStorage::memory()),
        #[cfg(all(feature = "image", feature = "resources"))]
        image_resource_cache: Mutex::new(std::collections::HashMap::new()),
        #[cfg(all(feature = "svg", feature = "resources"))]
        svg_resource_cache: Mutex::new(std::collections::HashMap::new()),
      }),
    }
  }

  pub fn menu_bar_support(&self) -> super::menu::MenuBarSupport {
    if cfg!(all(feature = "winit", target_os = "macos")) {
      super::menu::MenuBarSupport::Native
    } else {
      super::menu::MenuBarSupport::Unavailable
    }
  }
  /// Retains the model everywhere for headless QA; installs a native bar only on macOS.
  pub fn set_menu_bar(&self, bar: super::menu::MenuBar) {
    self.shared.menu.set(bar);
  }
  pub fn on_menu_activate(&self, handler: impl Fn(&str) + Send + Sync + 'static) {
    self.shared.menu.on_activate(handler);
  }
  pub fn menu_controller(&self) -> super::menu::MenuController {
    self.shared.menu.clone()
  }

  pub fn set_scale_override(&mut self, scale: Option<f32>) {
    *self.shared.scale_override.lock() = scale;
    self.shared.glyph_engine.lock().clear_cache();
  }

  #[cfg(feature = "tokio")]
  pub fn with_tokio_handle(self, handle: tokio::runtime::Handle) -> Self {
    *self.shared.tokio_handle.lock() = Some(handle);
    self
  }

  #[cfg(feature = "tokio")]
  pub fn set_tokio_handle(&mut self, handle: tokio::runtime::Handle) {
    *self.shared.tokio_handle.lock() = Some(handle);
  }

  #[cfg(feature = "tokio")]
  pub fn clear_tokio_handle(&mut self) {
    *self.shared.tokio_handle.lock() = None;
  }

  #[cfg(feature = "tokio")]
  pub(crate) fn tokio_handle(&self) -> Option<tokio::runtime::Handle> {
    self.shared.tokio_handle.lock().clone()
  }

  pub fn theme(&self) -> &Theme {
    &self.shared.theme
  }

  /// Handle for opening secondary OS windows (see [`WindowOpener`]).
  pub fn window_opener(&self) -> WindowOpener {
    self.shared.window_opener.clone()
  }

  #[cfg(feature = "i18n")]
  pub fn i18n(&self) -> &I18n {
    &self.shared.i18n
  }

  pub fn load_font(&mut self, data: Vec<u8>) {
    self.shared.glyph_engine.lock().load_font(data);
  }

  pub fn load_font_file(&mut self, path: &Path) {
    self.shared.glyph_engine.lock().load_font_file(path);
  }

  pub fn load_fonts_dir(&mut self, path: &Path) {
    self.shared.glyph_engine.lock().load_fonts_dir(path);
  }

  pub fn register_font(&mut self, name: &str, family: &str) {
    self.shared.glyph_engine.lock().register_font(name, family);
  }

  /// Installs multiple font faces and aliases with one text-cache invalidation.
  /// Existing atlas entries remain valid because fontdb assigns stable IDs to
  /// loaded faces, so only newly requested glyphs need to be rasterized.
  pub fn install_fonts<I, A, N, F>(&mut self, fonts: I, aliases: A)
  where
    I: IntoIterator<Item = Vec<u8>>,
    A: IntoIterator<Item = (N, F)>,
    N: AsRef<str>,
    F: AsRef<str>,
  {
    self.shared.glyph_engine.lock().install_fonts(fonts, aliases);
  }

  /// Prepares the fonts `styles` are drawn with, so that the first frame
  /// that draws them does not have to.
  ///
  /// The first text of each family, weight and slant makes the text engine
  /// score every font face on the system for it: 4 to 11 ms a combination on
  /// a Windows desktop, on the frame that first shows it. This does it now
  /// instead. Sizes, colours and spacing do not matter: a regular and a bold
  /// style of a family cover every size of them.
  ///
  /// It blocks for that long, so call it where that costs nothing: `App` is
  /// `Send`, and a clone of it on a thread of its own can warm the fonts
  /// while the window and the renderer are created. Call it after the fonts
  /// are loaded: loading a font forgets what was prepared.
  ///
  /// ```no_run
  /// # use lurq::{app::App, layout::text_style::{FontWeight, TextStyle}};
  /// let app = App::new();
  /// let warming = app.clone();
  /// std::thread::spawn(move || {
  ///   let bold = TextStyle {
  ///     weight: FontWeight::Bold,
  ///     ..TextStyle::default()
  ///   };
  ///   warming.warm_text_styles(&[TextStyle::default(), bold]);
  /// });
  /// ```
  pub fn warm_text_styles<'a>(&self, styles: impl IntoIterator<Item = &'a crate::layout::text_style::TextStyle>) {
    for style in styles {
      // A lock per style: a frame that wants the engine gets in between.
      self.shared.glyph_engine.lock().warm_style(style);
    }
  }

  #[cfg(feature = "resources")]
  pub fn set_resource_root(&mut self, root: PathBuf) {
    self.shared.resource_loader.lock().set_root(root);
  }

  #[cfg(feature = "persistent_storage")]
  /// Returns a shared handle to the currently selected storage backend.
  pub fn persistent_storage(&self) -> crate::persistent_storage::PersistentStorage {
    self.shared.persistent_storage.lock().clone()
  }

  #[cfg(feature = "persistent_storage")]
  pub fn set_persistent_storage_path(
    &mut self,
    path: impl Into<PathBuf>,
  ) -> Result<(), crate::persistent_storage::PersistentStorageError> {
    *self.shared.persistent_storage.lock() = crate::persistent_storage::PersistentStorage::open(path.into())?;
    Ok(())
  }

  #[cfg(feature = "persistent_storage")]
  pub fn persistent_value<T: crate::persistent_storage::PersistentValue>(&self, key: &str) -> Option<T> {
    self.persistent_storage().value(key)
  }

  #[cfg(feature = "persistent_storage")]
  pub fn set_persistent_value<T: crate::persistent_storage::IntoPersistentValue>(
    &self,
    key: &str,
    value: T,
  ) -> Result<(), crate::persistent_storage::PersistentStorageError> {
    self.persistent_storage().set_value(key, value)
  }

  #[cfg(feature = "persistent_storage")]
  pub fn read_bulk<I, K>(
    &self,
    keys: I,
  ) -> Result<crate::persistent_storage::PersistentReadBatch, crate::persistent_storage::PersistentStorageError>
  where
    I: IntoIterator<Item = K>,
    K: AsRef<str>,
  {
    self.persistent_storage().read_bulk(keys)
  }

  #[cfg(feature = "persistent_storage")]
  pub fn read_bulk_values<T, I, K>(
    &self,
    keys: I,
  ) -> Result<Vec<Option<T>>, crate::persistent_storage::PersistentStorageError>
  where
    T: crate::persistent_storage::PersistentValue,
    I: IntoIterator<Item = K>,
    K: AsRef<str>,
  {
    self.persistent_storage().read_bulk_values(keys)
  }

  #[cfg(feature = "persistent_storage")]
  pub fn write_bulk<I, E>(&self, entries: I) -> Result<(), crate::persistent_storage::PersistentStorageError>
  where
    I: IntoIterator<Item = E>,
    E: crate::persistent_storage::IntoPersistentWrite,
  {
    self.persistent_storage().write_bulk(entries)
  }
}

#[cfg(test)]
mod tests {
  use super::App;
  use crate::layout::text_style::{FontWeight, TextStyle};

  fn bold() -> TextStyle {
    TextStyle {
      weight: FontWeight::Bold,
      ..TextStyle::default()
    }
  }

  #[test]
  fn fonts_warmed_on_another_thread_measure_as_they_would() {
    let untouched = App::new();
    let warmed = App::new();
    let warming = warmed.clone();
    std::thread::spawn(move || warming.warm_text_styles(&[TextStyle::default(), bold()]))
      .join()
      .expect("the warming thread");

    for style in [TextStyle::default(), bold()] {
      for text in ["Ag", "Voice and text for the team", "wrapped\nlines"] {
        let expected = untouched
          .shared
          .glyph_engine
          .lock()
          .measure_text(text, &style, f32::MAX);
        let measured = warmed.shared.glyph_engine.lock().measure_text(text, &style, f32::MAX);
        assert_eq!(
          (measured.width, measured.height),
          (expected.width, expected.height),
          "{text:?}"
        );
      }
    }
  }

  #[test]
  fn warming_no_styles_does_nothing() {
    let app = App::new();
    app.warm_text_styles(&[]);
    let engine = app.shared.glyph_engine.lock();
    assert_eq!((engine.measure_hits, engine.measure_misses), (0, 0));
  }
}
