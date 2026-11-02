---
title: Futures And Timers
description: Async data fetching with futures, imperative actions, and timer-based scheduling.
---

# Futures And Timers

## Futures

`ctx.future` runs an async operation that automatically re-executes when its dependencies change. The returned `FutureHandle` exposes a reactive `Signal<FutureState<T, E>>`.

For requests whose data should be shared across components and retained across navigation, enable the `query` feature and use [Queries](../queries/). Queries add a shared cache, freshness, request deduplication, and invalidation to finite async reads.

Use `ctx.future` for finite async work that resolves to one result, such as loading a page, querying an endpoint, or submitting a request. Do not use it to manually chain a continuous subscription by changing a dependency after every completion. For watch receivers, sockets, event feeds, and other multi-item sources, use [`ctx.stream`](#streams).

```rust
use lurq::{
  app::{component::Component, ctx::{Ctx, FutureStatus}},
  components::Text,
  node::Element,
};

struct UserProfile;

impl Component for UserProfile {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self { Self }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let handle = ctx.future((), |_| async {
      Ok::<_, String>("loaded data".to_owned())
    });
    let state = handle.state().get();
    match state.status {
      FutureStatus::Pending => Text::new("Loading..."),
      FutureStatus::Fulfilled => Text::new(&state.data.unwrap()),
      FutureStatus::Rejected => Text::new(&format!("Error: {}", state.error.unwrap())),
      FutureStatus::Idle => Text::new(""),
    }
  }
}
```

### Dependency-Driven Re-execution

The first argument is a dependency value. When it changes between renders, the future restarts.

```rust
fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
  let page = self.page.get();
  let handle = ctx.future(page, |page| async move {
    fetch_page(page).await
  });
  // ...
}
```

The dependency must implement `Clone + PartialEq + Send + Sync + 'static`. Use a tuple to combine multiple deps.

`ctx.future` restarts when the dependency value is different on a later render. If a future result is consumed and that render then changes the dependency, the new future is only created by a following render. That is fine for finite request/retry flows, but it is the wrong shape for continuous streams.

### FutureState

| Field | Type | Description |
| --- | --- | --- |
| `status` | `FutureStatus` | `Idle`, `Pending`, `Fulfilled`, or `Rejected`. |
| `data` | `Option<T>` | The last value that succeeded. Kept while a new run is `Pending` and after it is `Rejected`. |
| `error` | `Option<E>` | The error of the latest run while `Rejected`. |

Convenience methods: `is_idle()`, `is_pending()`, `is_fulfilled()`, `is_rejected()`, and:

| Method | Returns |
| --- | --- |
| `outcome()` | `Option<Result<&T, &E>>`: how the latest run ended. `Some(Ok(value))` when fulfilled, `Some(Err(error))` when rejected, `None` while idle or pending. |
| `fulfilled_data()` | `Option<&T>`: the value of the latest run if it was fulfilled. |

`data` is not the result of the latest run. A run that fails after an earlier one succeeded is `Rejected` with `error` set, and `data` still holds the earlier value, so a view can keep showing it next to the error. Code that decides whether the latest run succeeded, such as a `ctx.watch` that closes a dialog after a save, must not test "has `data` and is not pending": that is true after a failed save too. Use `outcome()`, `fulfilled_data()`, `is_fulfilled()`, or match on `status`:

```rust
ctx.watch(&save.state(), move |state| match state.outcome() {
  Some(Ok(_)) => close.set(true),
  Some(Err(error)) => show_error(error),
  None => {}
});
```

### FutureHandle

| Method | Description |
| --- | --- |
| `.state()` | Returns a `Signal<FutureState<T, E>>` for reactive reads. |
| `.cancel()` | Cancels the in-flight future. |
| `.is_active()` | Whether a future is currently running. |

## Streams

`ctx.stream` runs a continuous async producer. The producer receives a `StreamEmitter<T, E>` and can call `.emit(value)` repeatedly. The returned `StreamHandle` exposes the same reactive `Signal<FutureState<T, E>>` shape as futures, but the task stays alive after each emitted item.

Use streams for `watch::Receiver`, websocket subscriptions, event feeds, filesystem watchers, and other sources that can produce more than one value.

The example below uses a Tokio watch channel. Add a direct `tokio` dependency with the `sync` feature. Wrap the receiver in props that satisfy `PartialEq` and DevTools inspection bounds; increment `source_id` when replacing its channel so the stream restarts.

```rust
use lurq::{
  app::{
    component::Component,
    ctx::{Ctx, FutureStatus, StreamEmitter},
  },
  components::Text,
  node::Element,
};
use tokio::sync::watch;

struct ServerEvents;

#[derive(Clone, lurq::DevtoolsInspectable)]
struct ServerEventsProps {
  source_id: u64,
  #[devtools_ignore]
  receiver: watch::Receiver<String>,
}

impl PartialEq for ServerEventsProps {
  fn eq(&self, other: &Self) -> bool {
    self.source_id == other.source_id
  }
}

impl Component for ServerEvents {
  type Props = ServerEventsProps;

  fn create(_ctx: &mut Ctx) -> Self { Self }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let receiver = props.receiver;
    let handle = ctx.stream(props.source_id, move |_, emitter: StreamEmitter<String, String>| {
      let mut receiver = receiver.clone();
      async move {
        loop {
          if receiver.changed().await.is_err() {
            break;
          }
          if !emitter.emit(receiver.borrow().clone()) {
            break;
          }
        }
      }
    });

    let state = handle.state().get();
    match state.status {
      FutureStatus::Fulfilled => Text::new(&state.data.unwrap()),
      FutureStatus::Pending => Text::new("Waiting..."),
      FutureStatus::Rejected => Text::new(&format!("Stream error: {}", state.error.unwrap())),
      FutureStatus::Idle => Text::new(""),
    }
  }
}
```

### StreamHandle

| Method | Description |
| --- | --- |
| `.state()` | Returns a `Signal<FutureState<T, E>>` containing the latest emitted item or error. |
| `.cancel()` | Cancels the stream task. |
| `.is_active()` | Whether the stream task is currently running. |

### StreamEmitter

| Method | Description |
| --- | --- |
| `.emit(value)` | Publishes a fulfilled item to the UI state. Returns `false` if the receiver was dropped. |
| `.reject(error)` | Publishes a rejected state while keeping the stream task alive. Returns `false` if the receiver was dropped. |

The dependency argument works the same way as `ctx.future`: changing it between renders cancels the existing stream task and starts a new one.

## Future Actions

`ctx.future_action` creates a future that does not run automatically. Instead, you call `.run(args)` to start it — useful for form submissions, button-triggered requests, or any operation that should not run on every render.

```rust
fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
  let action = ctx.future_action(|query: String| async move {
    search(query).await
  });
  let state = action.state().get();

  Column::new()
    .child(Button::new("Search").on_click({
      let action = action.clone();
      move |_| action.run("lurq".to_owned())
    }))
    .child(match state.status {
      FutureStatus::Idle => Text::new("Press search"),
      FutureStatus::Pending => Text::new("Searching..."),
      FutureStatus::Fulfilled => Text::new(&state.data.unwrap()),
      FutureStatus::Rejected => Text::new("Failed"),
    })
}
```

`FutureAction` has the same `.state()`, `.cancel()`, and `.is_active()` methods as `FutureHandle`, plus `.run(args)` and `.run_if_idle(args)`.

`.run(args)` always starts a run: a run still in flight is cancelled and its result is never applied. A double click on a button wired to `.run` therefore aborts the first request and keeps only the second. When a run in flight must finish, start with `.run_if_idle(args)`: it starts nothing while a run is in flight (`.is_active()`), and returns whether it started.

```rust
Button::new("Save").on_click({
  let save = save.clone();
  move |_| {
    save.run_if_idle(draft.get_untracked());
  }
})
```

An action does not have to be created in `render`. Created in `create`, it gets a slot of its own that lives until the component unmounts, so it can be kept in the component struct and run from any render or handler:

```rust
struct SearchBox {
  search: FutureAction<String, Vec<Hit>, String>,
}

impl Component for SearchBox {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      search: ctx.future_action(|query: String| async move { search(query).await }),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let search = self.search.clone();
    Button::new("Search").on_click(move |_| search.run("lurq".to_owned()))
  }
}
```

`ctx.future` and `ctx.stream` called in `create` work the same way: they start once, with the `deps` given there, and run until they finish, are cancelled, or the component unmounts.

A `ctx.watch` on `action.state()` may call `.run(args)` again, for example to retry when the state becomes `Rejected`. The new run sets the state to `Pending` after the current notification. See [writes from callbacks](../reactivity/#writes-from-callbacks).

When using the `form` feature, `FormProps::submit_action(action)` wires a `FutureAction<FormValues, _, FormErrors>` into a mounted form. It validates before running the action, exposes `form.submitting()`, blocks duplicate submits while pending, and maps rejected `FormErrors` back into field errors.

## Task Lifetime

Futures, streams, and future actions belong to the component that creates them. Calls made in `render` are positional, like hooks: each render must make the same `ctx.future`, `ctx.stream`, and `ctx.future_action` calls in the same order, and a call is matched with the slot of the same position in the previous render. Calls made in `create` get slots of their own, which no render replaces or drops. A task is cancelled when:

- its dependency changes (the new task replaces it),
- `.cancel()` is called,
- a render no longer reaches its call: the component makes fewer `ctx.future`, `ctx.stream`, and `ctx.future_action` calls than before, or a different one at that position,
- its component unmounts, which includes mounting another root and dropping the `Tree`.

A cancelled task's result is never applied. With a Tokio handle, cancelling aborts the Tokio task: it stops the next time it yields to the runtime, at an `.await` whose future is not ready yet, and the runtime drops its future, so a stream does not have to reach its next `emit` to stop. Code between such points keeps running until it yields: CPU-bound work, or a loop whose `.await`s are always ready, is not interrupted. `.run(args)` on a `FutureAction` whose component has unmounted does nothing. `.run(args)` on an action that a render dropped (its component is still mounted, but a render no longer made the call at its position) does nothing either, and logs an error through `tracing` that names the component: that is a bug in the component, fixed by creating the action in `create` or by making the calls unconditionally.

An offstage component (`ctx.mount_offstage`, `Router::mount_offstage`) is still mounted, so its tasks are not cancelled, but its futures are not polled until it is active again. A future or stream polled cooperatively (without a Tokio handle) waits. A task already running on Tokio keeps running, and what it produced while offstage is applied, in order, once the component is active again. Removing an offstage component unmounts it and cancels its tasks.

## Tokio Integration

Enable `tokio` and configure a live runtime handle to spawn futures and streams on Tokio. Add a direct Tokio dependency when application code uses its APIs:

```toml
lurq = { version = "0.53.0", features = ["tokio"] }
tokio = { version = "1", features = ["rt-multi-thread", "sync", "time", "net"] }
```

Pass a tokio handle when creating the `App`:

```rust
let tokio_rt = tokio::runtime::Runtime::new().unwrap();
let app = App::new().with_tokio_handle(tokio_rt.handle().clone());
```

With a tokio handle, futures and streams spawn onto the tokio runtime and complete independently. Results are delivered back to the UI thread on the next `tree.tick_futures()` call.

Keep `tokio_rt` alive for as long as the UI uses its tasks. Without a configured Tokio handle, futures and streams are polled cooperatively during `tree.tick_futures()`, even if the Cargo feature is enabled. Tokio I/O and timers require an appropriately configured runtime.

## Waking the Event Loop

An idle window does not poll: its event loop sleeps until the next timer or scheduled frame, an input event, or a wake (since lurq 0.45.0). Work that finishes off the loop wakes it:

- A result or stream item handed over from Tokio wakes the loop when it arrives.
- A future that lurq polls itself (one not running on Tokio) gets a waker that wakes the loop. Such a future must honour the `Future` contract: when it returns `Pending`, it must arrange for that waker to be called once it can make progress (an async channel, timer or I/O future does this for you). A future that returns `Pending` without arranging a wake is not polled again until something else wakes the loop.
- Starting or restarting a timer, or running a future action, from another thread wakes the loop.
- A `Signal` written off the loop's thread wakes the loop when the write marks a component for render: a `.set` inside a Tokio `ctx.future`, from a `std::thread`, or a `StreamEmitter::emit` called from a thread. The component renders the new value on the loop's next turn. Writes wake the loop once until it renders them, so a thread that writes thousands of times between two turns wakes it once and the render shows the latest value. A write on the loop's own thread needs no wake: the turn it happens in picks it up.

## Timers

### Timeout

A one-shot timer that fires once after a duration.

```rust
use std::time::Duration;

fn create(ctx: &mut Ctx) -> Self {
  let count = ctx.signal(0);
  let timeout = ctx.create_timeout(Duration::from_secs(2), {
    let count = count.clone();
    move || count.update(|n| *n += 1)
  });
  timeout.start();
  Self { count, _timeout: timeout }
}
```

| Method | Description |
| --- | --- |
| `.start()` | Arm the timer. If already armed, does nothing. |
| `.restart()` | Reset the deadline to `now + duration`. |
| `.cancel()` | Stop the timer without firing. |
| `.is_active()` | Whether the timer is armed. |

A timeout fires at most once. After firing it becomes inactive.

### Interval

A repeating timer that fires every `duration`.

```rust
let interval = ctx.create_interval(Duration::from_millis(500), {
  let tick = tick.clone();
  move || tick.update(|n| *n += 1)
});
interval.start();
```

| Method | Description |
| --- | --- |
| `.start()` | Arm the interval. |
| `.restart()` | Reset the next fire to `now + duration`. |
| `.stop()` | Stop repeating. |
| `.is_active()` | Whether the interval is running. |

### Lifecycle

Create timers in `Component::create` and store them in the component struct. The runtime ticks timers each frame via `tree.tick_timers()`. When a timer fires it invokes its callback, which typically updates a signal, causing a re-render.
