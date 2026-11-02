//! Main-thread AppKit integration test (the ordinary Rust test harness runs tests on worker threads): a screenshot of
//! a window nobody can see. AppKit hands a hidden application's windows no drawable, so the frame a capture waits for
//! is drawn offscreen. Runs on macOS; other hosts do nothing.
#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
  macos::run();
}

#[cfg(target_os = "macos")]
mod macos {
  use std::{
    path::PathBuf,
    sync::{
      Arc, OnceLock,
      atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
  };

  use lurq::{
    app::{
      App, Tree, WindowHandle, component::Component, ctx::Ctx, wgpu_render::WgpuRenderEngine, winit_shell::WinitWindow,
    },
    components::Text,
    node::Element,
  };
  use objc2_app_kit::{NSApplication, NSWindowOcclusionState};
  use objc2_foundation::MainThreadMarker;

  const TITLE: &str = "lurq occluded capture";
  const SIZE: (u32, u32) = (300, 180);

  static WINDOW: OnceLock<WindowHandle> = OnceLock::new();

  struct Root;
  impl Component for Root {
    type Props = ();
    fn create(ctx: &mut Ctx) -> Self {
      assert!(WINDOW.set(ctx.window()).is_ok());
      Self
    }
    fn render(&self, _: &mut Ctx) -> impl Into<Element> {
      Text::new("Captured while hidden")
    }
  }

  #[derive(Debug)]
  enum Phase {
    /// The window draws its first frames.
    Showing,
    /// The application is hidden; AppKit has yet to report the window not visible.
    Hiding,
    /// The screenshot was requested while the window could not be seen.
    Capturing,
  }

  pub fn run() {
    let output = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("occluded_capture.png");
    if output.exists() {
      std::fs::remove_file(&output).expect("an earlier run's screenshot can be removed");
    }
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.set_render_engine_factory(|| Box::new(WgpuRenderEngine::new()));
    tree.mount_root::<Root>(&mut app, ());

    // A window command wakes the event loop: setting the same title keeps the ticks coming while nothing else
    // happens, as while the capture is read back.
    let done = Arc::new(AtomicBool::new(false));
    let nudger = {
      let done = done.clone();
      thread::spawn(move || {
        while !done.load(Ordering::SeqCst) {
          thread::sleep(Duration::from_millis(50));
          if let Some(window) = WINDOW.get() {
            window.set_title(TITLE);
          }
        }
      })
    };

    let started = Instant::now();
    let mut phase = Phase::Showing;
    let captured = output.clone();
    WinitWindow::new(app, tree)
      .with_title(TITLE)
      .with_size(SIZE.0, SIZE.1)
      .on_tick(move |_, _| {
        assert!(
          started.elapsed() < Duration::from_secs(15),
          "occluded capture timed out while {phase:?}"
        );
        let app = NSApplication::sharedApplication(MainThreadMarker::new().expect("ticks run on the main thread"));
        let window = WINDOW.get().expect("the root component stored its window");
        match phase {
          Phase::Showing if started.elapsed() > Duration::from_millis(500) => {
            app.hide(None);
            phase = Phase::Hiding;
          }
          Phase::Hiding => {
            let visible = app
              .windows()
              .objectAtIndex(0)
              .occlusionState()
              .contains(NSWindowOcclusionState::Visible);
            if !visible {
              window.screenshot(&captured);
              phase = Phase::Capturing;
            }
          }
          Phase::Capturing if image::open(&captured).is_ok() => {
            assert!(app.isHidden(), "the application stayed hidden through the capture");
            window.close();
          }
          _ => {}
        }
      })
      .run();
    done.store(true, Ordering::SeqCst);
    nudger.join().expect("the nudging thread does not panic");

    let image = image::open(&output).expect("the screenshot was written").to_rgba8();
    assert!(
      image.width() >= SIZE.0 && image.height() >= SIZE.1,
      "the capture has the window's size"
    );
    let first = image.get_pixel(0, 0);
    assert!(
      image.pixels().any(|pixel| pixel != first),
      "the capture holds the drawn frame, not a blank texture"
    );
    println!("a screenshot of a hidden application's window was drawn offscreen and written");
  }
}
