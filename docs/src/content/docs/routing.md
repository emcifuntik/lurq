---
title: Routing
description: Declarative routes, nested layouts, links, params, guards, and navigation.
---

# Routing

Requires the `router` feature flag.

```toml
lurq = { version = "0.60.0", features = ["router"] }
```

The router is an in-process UI router. It does not depend on browser URLs; it owns a reactive path signal and renders the matching route tree inside your `Tree`.

Use it when an app has multiple screens, a persistent shell, nested content areas, or navigation history.

## Minimal Router

Create a `RouterHandle` in `create` with `ctx.router`, then render it with `Router::mount`.

```rust
use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Router, Text},
  node::Element,
  router::{RouterHandle, Routes},
};

struct AppRoot {
  router: RouterHandle,
}

impl Component for AppRoot {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let router = ctx.router(
      Routes::new()
        .route("/", |_ctx| Text::new("Home").into())
        .route("/settings", |_ctx| Text::new("Settings").into()),
    );

    router.replace("/");
    Self { router }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Router::mount(ctx, self.router.clone())
  }
}
```

`ctx.router` creates the handle once for the component. Call `replace("/")` or `push("/")` during `create` to set the initial route.

## Layout Routes

Use `layout` when several routes share a shell. Render `Outlet::mount(ctx)` where the child route should appear.

```rust
use lurq::{
  app::ctx::Ctx,
  components::{Column, Link, Outlet, Text},
  node::Element,
  router::Routes,
};

fn routes() -> Routes {
  Routes::new().layout(
    "/",
    |ctx| app_shell(ctx),
    |routes| {
      routes
        .route("/", |_ctx| Text::new("Dashboard").into())
        .route("/profile", |_ctx| Text::new("Profile").into())
        .route("/settings", |_ctx| Text::new("Settings").into())
    },
  )
}

fn app_shell(ctx: &mut Ctx) -> Element {
  Column::new()
    .child(Link::build(ctx, "Dashboard", "/"))
    .child(Link::build(ctx, "Profile", "/profile"))
    .child(Link::build(ctx, "Settings", "/settings"))
    .child(Outlet::mount(ctx))
    .into()
}
```

Nested layouts work the same way: a child layout can render another `Outlet`.

## A Stack Of Screens

`StackOutlet` goes where an `Outlet` would, and moves between screens as a phone's apps do. It is added after
**lurq 0.60.0**. An `Outlet` shows the current route and drops the one before; a `StackOutlet` keeps every
entry of the history, from the first to the current one, as a screen of its own:

- a `push` brings the new screen in from the right, over the one before;
- `back` takes the top screen out to the right and unmounts it;
- a screen under the top one stays mounted, with its state and its scroll, and is not laid out or painted
  until it shows again;
- on a touch screen a finger in from the left edge drags the top screen, and takes it back when lifted past
  half the width or fast enough;
- a `replace` swaps the top screen with no move.

```rust
use lurq::{
  app::ctx::Ctx,
  components::{NavigationBar, NavigationItem, StackOutlet, StackOutletProps, Text},
  node::Element,
  router::Routes,
};

fn routes() -> Routes {
  Routes::new().layout("/", shell, |routes| {
    routes
      .route("/", |ctx| {
        ctx.navigation_item(NavigationItem::new("Inbox"));
        Text::new("Inbox").into()
      })
      .route("/message/:id", |ctx| {
        ctx.navigation_item(NavigationItem::new("Message"));
        ctx.mount::<MessageScreen>(())
      })
  })
}

fn shell(ctx: &mut Ctx) -> Element {
  StackOutlet::mount_with(ctx, StackOutletProps::new().bar(NavigationBar::new()))
}
```

Give the history its first entry in `create` with `router.replace("/")`: it is the screen at the bottom of the
stack, the one the others go back to.

`StackOutlet::mount(ctx)` is the stack alone. With `StackOutletProps::bar` it draws a navigation bar over its
screens, under the safe area's top, 44 logical pixels high and 32 in a window lower than 480, as a phone held
on its side is: the top screen's title, and the way back with the title of the screen
under it. The titles move with the screens, and with the finger. `NavigationBar::new()` is a light bar and
`NavigationBar::dark()` a dark one; `background`, `tint`, `title` and `border` set its colors.

A screen says what the bar shows for it by calling `ctx.navigation_item` in its `render`, or in its route:

| `NavigationItem` | |
|------------------|--|
| `new(title)` | The title in the middle of the bar |
| `back_title(text)` | What the way back says on the screen pushed over this one, in place of the title |
| `back_gesture(false)` | A finger from the edge does not take this screen back; the bar's way back still does |
| `leading(element)` | What is at the bar's leading end in place of the way back |
| `trailing(element)` | What is at the bar's trailing end while this screen is on top |
| `title_view(element)` | What stands in the middle of the bar in place of the title |
| `bar_hidden(true)` | No bar over this screen: it reaches the top of the outlet, under the status bar |
| `large_title(true)` | The title large at the top of the content, scrolling away with it |


A screen with a large title puts its content in `StackOutlet::large_title_scroll(ctx, content)`, which draws
the title over the content and scrolls the two as one. While the large title is in view the bar shows no title
and no color of its own; when the title has scrolled away the bar's title and color come in. Pulled down
past its top, the content takes the title with it, and the title grows a little. When a screen is pushed over
one whose large title is in view, that title travels to the new screen's way back, shrinking into its label, and
back again when the screen is taken off.
`StackOutlet::screen_scroll(ctx)` gives that list's `ScrollState`.

```rust
ctx.navigation_item(NavigationItem::new("Inbox").large_title(true));
StackOutlet::large_title_scroll(ctx, messages)
```

For a bar that is the app's own altogether, give the outlet a builder with `StackOutletProps::bar_with`. It
is called with the state of the stack and returns the bar, which the outlet puts across its top:

```rust
let bar: NavigationBarBuilder = Arc::new(|state: &NavigationBarState| {
  Row::new()
    .padding_top(state.safe_area.top)
    .height(state.safe_area.top + 52.0)
    .child(
      Text::new(&state.top.item.title)
        // 0 in place, 1 gone: the title fades as its screen goes out, in step with it.
        .opacity(1.0 - state.out)
        .transition(Transition::opacity().duration(state.duration).easing(state.easing)),
    )
    .into()
});
StackOutlet::mount_with(ctx, StackOutletProps::new().bar_with(bar.clone()))
```

`state.top` is the screen in front and `state.under` the one behind it while they move, each with its
`NavigationItem` and what the way back from it says. `state.out` is how far out of view the front screen is;
with a transition of `state.duration` and `state.easing` a part of the bar moves with the screens, and follows
a finger, for which the duration is zero. Make the builder once, in `create`: a new one at each render renders
the outlet again.

A screen comes in and goes out on a spring, `Easing::Spring`, and one let go by a finger goes on at the
finger's speed.

A screen of a stack reads its own entry: `ctx.route_params()` and `ctx.route_state()` give what the screen was
opened with, whatever was pushed over it since. `ctx.route_path()` is the router's current path on every
screen.

How the screens and the bar move is `StackMotion`, given with `StackOutletProps::motion`. Its defaults were
set by eye beside the platform's:

```rust
StackOutletProps::new().motion(StackMotion {
  duration: Duration::from_millis(400),
  ..StackMotion::default()
})
```

| `StackMotion` | Default | |
|---------------|---------|--|
| `duration` | 500 ms | How long a screen takes to come in or go out, on a spring |
| `parallax` | 0.3 | How far the screen under gives way, as a share of the width |
| `dim` | 0.1 | How dark the screen under gets |
| `back_share` | 0.5 | How far across a finger takes the top screen for it to go back |
| `back_speed` | 500 | The speed of the finger that takes it back from anywhere, in logical pixels a second |
| `title_fade` | 200 ms | How long the bar's title and color take to come and go under a large title |
| `large_title_growth` | 1.1 | How much larger a large title gets when pulled down |
| `large_title_pull` | 150 | The pull, in logical pixels, that takes it there |

In the bar, a title too long for it ends in an ellipsis; the way back says `Back` when the title of the screen
under has no room, and is its arrow alone when `Back` has none. A window lower than 480 logical pixels, a phone
on its side, has no large titles: the title is in the bar. What a screen gives as `leading`, `trailing` or
`title_view` is shown as the screen last rendered it.

`StackOutletProps::background` paints behind each screen; a screen with no background of its own shows the
one under it through while it moves.

## Links

`Link` reads the router navigator from context, so it must be rendered inside `Router::mount`.

```rust
Link::build(ctx, "Settings", "/settings")
```

For custom link contents, use `build_empty` and attach children:

```rust
Link::build_empty(ctx, "/settings")
  .child(Text::new("Open settings"))
```

Links navigate with `push`, so they add a history entry.

## Programmatic Navigation

Read the current navigator with `ctx.navigator()`.

```rust
if let Some(navigator) = ctx.navigator() {
  navigator.push("/profile");
  navigator.replace("/login");
  navigator.back();
  navigator.forward();
}
```

Use `push` for normal navigation and `replace` for redirects, login callbacks, initial route setup, or cases where the old path should not stay in history.

The current path is reactive:

```rust
let path = ctx.route_path();
```

Reading it during render marks the component as dependent on route changes.

## Params

Use `:name` in a route pattern to capture one path segment.

```rust
Routes::new().route("/users/:id", |ctx| ctx.mount::<UserPage>(()))
```

Inside the route component, read params with `ctx.route_params()`.

```rust
use lurq::{
  app::{component::Component, ctx::Ctx},
  components::Text,
  node::Element,
};

struct UserPage;

impl Component for UserPage {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let params = ctx.route_params();
    let id = params.get("id").unwrap_or("unknown");
    Text::new(&format!("User {id}"))
  }
}
```

`Params::get_parsed` parses directly into any type that implements `FromStr`.

```rust
let id: Option<u64> = ctx.route_params().get_parsed("id");
```

## Pattern Rules

Patterns are normalized around path segments:

| Pattern | Matches | Notes |
|---------|---------|-------|
| `/` | `/` | Root route |
| `/settings` | `/settings` | Static segments are case-sensitive |
| `/users/:id` | `/users/42` | Params capture one non-empty segment |
| `/files/*` | `/files/readme` | Wildcard matches one segment and does not capture |
| `/docs/**rest` | `/docs/a/b/c` | Catch-all captures remaining segments |

Specific routes win over broader routes. For example, `/users/settings` is preferred over `/users/:id` when both are present.

## Fallback Routes

Use `fallback` for unmatched paths.

```rust
Routes::new()
  .route("/", |_ctx| Text::new("Home").into())
  .fallback(|_ctx| Text::new("Not found").into())
```

Fallbacks can also be used inside a layout route so the shared shell remains visible for unknown child paths.

## Guards

Attach a guard to the route defined immediately before it.

```rust
use lurq::{
  components::Text,
  router::{GuardAction, Routes},
};

let authenticated = false;

let routes = Routes::new()
  .route("/login", |_ctx| Text::new("Login").into())
  .route("/account", |_ctx| Text::new("Account").into())
  .guard(move |_route| {
    if authenticated {
      GuardAction::Allow
    } else {
      GuardAction::Redirect("/login".to_owned())
    }
  });
```

Guards can:

- `Allow` the navigation
- `Deny` it and keep the current path
- `Redirect(path)` to another route

Use guards for coarse route access rules. Keep field validation and form submission logic inside components.

## Testing Router Flows

Router handles are cloneable, so tests can keep a copy and drive navigation directly.

```rust
let router = ctx.router(
  Routes::new()
    .route("/", |_ctx| Text::new("Home").into())
    .route("/about", |_ctx| Text::new("About").into()),
);

router.push("/about");
```

For link behavior, send pointer down/up through the tree. Click events are synthesized by the tree from matching pointer press and release events.

```rust
tree.mouse_down(x, y, MouseButton::Left);
tree.mouse_up(x, y, MouseButton::Left);
```
