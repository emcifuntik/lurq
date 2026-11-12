use super::*;

#[cfg(test)]
// Renaming it would rename every test in the file.
#[allow(clippy::module_inception)]
mod tests {
  use super::*;

  #[test]
  fn window_icon_from_rgba_stores_dimensions_and_pixels() {
    let icon = WindowIcon::from_rgba(vec![255, 0, 0, 255], 1, 1);

    assert_eq!(icon.width(), 1);
    assert_eq!(icon.height(), 1);
    assert_eq!(icon.rgba(), &[255, 0, 0, 255]);
  }

  #[test]
  #[should_panic]
  fn window_icon_from_rgba_rejects_wrong_pixel_count() {
    WindowIcon::from_rgba(vec![255, 0, 0], 1, 1);
  }

  /// An MCP click on an app's Paste menu item runs its handler inside
  /// synthetic input; the command remembers that, and the shell refuses to
  /// read the person's clipboard for it.
  #[cfg(feature = "clipboard")]
  #[test]
  fn a_paste_request_made_under_synthetic_input_is_marked_and_refused() {
    let window = Window::new();
    let handle = window.handle();

    handle.paste_from_clipboard();
    crate::app::events::with_synthetic_input(|| handle.paste_from_clipboard());

    assert_eq!(
      window.take_commands(),
      vec![
        WindowCommand::PasteFromClipboard { synthetic: false },
        WindowCommand::PasteFromClipboard { synthetic: true },
      ]
    );

    // What the shell does with the synthetic one: nothing, not even a read.
    let reads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    struct Counting(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl crate::app::ClipboardReader for Counting {
      fn read_text(&self) -> Option<String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Some("secret".into())
      }

      fn read(&self) -> Option<crate::app::events::ClipboardContent> {
        self.read_text().map(crate::app::events::ClipboardContent::text)
      }
    }
    let value = crate::core::Signal::new(String::new());
    let mut tree = crate::app::Tree::new();
    tree.set_clipboard_reader(Some(std::sync::Arc::new(Counting(reads.clone()))));
    tree.set_root(crate::components::TextInput::new(value.clone()).id("input"));
    tree.pass_headless(&mut crate::app::App::new());
    tree.get_element_by_id_mut("input").unwrap().focus();

    assert!(!tree.paste_from_os_clipboard(true));
    assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(value.get(), "");

    assert!(tree.paste_from_os_clipboard(false));
    assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(value.get(), "secret");
  }

  #[test]
  fn window_handle_queues_title_bar_and_icon_commands() {
    let window = Window::new();
    let handle = window.handle();
    let color = Color::from_hex("#101215");
    let icon = WindowIcon::from_rgba(vec![255, 0, 0, 255], 1, 1);

    handle.set_title_bar_color(color);
    handle.set_icon(icon.clone());
    handle.set_corner_radius(WindowCornerRadius::RoundedSmall);
    handle.set_border_color(WindowBorderColor::None);
    handle.clear_title_bar_color();
    handle.clear_icon();
    handle.reset_corner_radius();

    assert_eq!(
      window.take_commands(),
      vec![
        WindowCommand::SetTitleBarColor(Some(color)),
        WindowCommand::SetIcon(Some(icon)),
        WindowCommand::SetCornerRadius(WindowCornerRadius::RoundedSmall),
        WindowCommand::SetBorderColor(WindowBorderColor::None),
        WindowCommand::SetTitleBarColor(None),
        WindowCommand::SetIcon(None),
        WindowCommand::SetCornerRadius(WindowCornerRadius::Default),
      ]
    );
  }
}

#[cfg(test)]
mod title_tests {
  use super::*;

  #[test]
  fn set_title_queues_changes_only() {
    let window = Window::new();
    let handle = window.handle();
    assert_eq!(handle.title(), None);

    handle.set_title("Orchester - Tasks");
    handle.set_title("Orchester - Tasks");
    handle.set_title(String::from("Orchester - Runs"));

    assert_eq!(handle.title().as_deref(), Some("Orchester - Runs"));
    assert_eq!(
      window.take_commands(),
      vec![
        WindowCommand::SetTitle("Orchester - Tasks".into()),
        WindowCommand::SetTitle("Orchester - Runs".into()),
      ]
    );
    handle.set_title("Orchester - Runs");
    assert!(window.take_commands().is_empty());
  }

  #[test]
  fn initial_title_does_not_replace_an_app_title() {
    let window = Window::new();
    window.record_initial_title("lurq");
    assert_eq!(window.handle().title().as_deref(), Some("lurq"));
    window.handle().set_title("lurq");
    assert!(window.take_commands().is_empty());

    let window = Window::new();
    window.handle().set_title("From the app");
    window.record_initial_title("From the builder");
    assert_eq!(window.handle().title().as_deref(), Some("From the app"));
    assert_eq!(
      window.take_commands(),
      vec![WindowCommand::SetTitle("From the app".into())]
    );
  }

  #[test]
  fn set_title_wakes_the_event_loop() {
    let window = Window::new();
    let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = wakes.clone();
    window.set_waker(Arc::new(move || {
      count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }));
    window.handle().set_title("a");
    window.handle().set_title("a");
    assert_eq!(wakes.load(std::sync::atomic::Ordering::SeqCst), 1);
  }
}

#[cfg(test)]
mod close_tests {
  use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
  };

  use super::*;

  #[test]
  fn close_veto_cancel_drop_and_delayed_proceed() {
    let window = Window::new();
    window.handle().on_close_requested(|request| {
      assert_eq!(request.source(), CloseRequestSource::Os);
      request.cancel();
    });
    window.dispatch_close_request(CloseRequestSource::Os);
    assert!(window.take_shell_commands().is_empty());
    window.handle().on_close_requested(drop);
    window.dispatch_close_request(CloseRequestSource::Os);
    assert!(window.take_shell_commands().is_empty());
    let pending = Arc::new(Mutex::new(None));
    let save = pending.clone();
    window
      .handle()
      .on_close_requested(move |request| *save.lock().unwrap() = Some(request));
    window.handle().request_close();
    assert!(window.take_shell_commands().is_empty());
    let request = pending.lock().unwrap().take().unwrap();
    assert_eq!(request.source(), CloseRequestSource::App);
    std::thread::spawn(move || request.proceed()).join().unwrap();
    assert_eq!(window.take_shell_commands(), vec![WindowCommand::Close]);
  }

  #[test]
  fn close_bypasses_handler_and_no_handler_preserves_behavior() {
    let window = Window::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    window.handle().on_close_requested(move |_| {
      count.fetch_add(1, Ordering::SeqCst);
    });
    window.handle().close();
    assert_eq!(window.take_shell_commands(), vec![WindowCommand::Close]);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    window.handle().clear_close_requested_handler();
    for source in [CloseRequestSource::Os, CloseRequestSource::App] {
      window.dispatch_close_request(source);
      assert_eq!(window.take_shell_commands(), vec![WindowCommand::Close]);
    }
  }

  #[test]
  fn proceed_wakes_event_loop_and_request_does_not_own_window() {
    let window = Window::new();
    let pending = Arc::new(Mutex::new(None));
    let save = pending.clone();
    window
      .handle()
      .on_close_requested(move |r| *save.lock().unwrap() = Some(r));
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = wakes.clone();
    window.set_waker(Arc::new(move || {
      count.fetch_add(1, Ordering::SeqCst);
    }));
    window.dispatch_close_request(CloseRequestSource::Os);
    pending.lock().unwrap().take().unwrap().proceed();
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    window.take_commands();
    window.dispatch_close_request(CloseRequestSource::Os);
    let stale = pending.lock().unwrap().take().unwrap();
    drop(window);
    stale.proceed();
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
  }

  #[test]
  fn reentrant_requests_are_deferred_and_registration_is_reentrant() {
    let window = Window::new();
    let handle = window.handle();
    let reentrant = handle.clone();
    handle.on_close_requested(move |r| {
      reentrant.clear_close_requested_handler();
      reentrant.request_close();
      r.cancel();
    });
    handle.request_close();
    assert!(window.take_shell_commands().is_empty());
    assert_eq!(window.take_shell_commands(), vec![WindowCommand::Close]);
  }
}
