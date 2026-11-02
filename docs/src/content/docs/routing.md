---
title: Routing
description: Declarative routes, nested layouts, links, params, guards, and navigation.
---

# Routing

Requires the `router` feature flag.

```toml
lurq = { version = "0.53.0", features = ["router"] }
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
