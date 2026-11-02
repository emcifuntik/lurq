---
title: Queries
description: Shared async data through named query functions, reactive handles, and typed invalidation.
---

# Queries

Enable the optional `query` feature:

```toml
lurq = { version = "0.53.0", features = ["query"] }
```

A query is a named async read whose arguments identify a cached result. Components observing the same query share data and a running request. The client retains results when components unmount, so returning to a screen can reuse its data.

## Define a query

```rust
#[lurq::query(stale_time = "30s")]
async fn get_user(id: u64) -> Result<User, ApiError> {
  api::get_user(id).await
}
```

`User`, `ApiError`, and `api` are application types and services. The query body can read HTTP, a database, files, or IPC; the feature does not add a transport dependency.

The macro makes `get_user(id)` a descriptor constructor. Calling it does not execute the body or start a request. The descriptor is observed through `ctx.query(...)`; ordinary async callers can continue to call the underlying `api::get_user(id).await` service.

Query definitions currently support safe, non-generic async free functions with named, owned arguments and a `Result<T, E>` return type. Arguments require `Clone + Eq + Hash + Send + Sync + 'static`. Values and errors require `Send + Sync + 'static`; they do not require `Clone`, `PartialEq`, or `DevtoolsInspectable`.

The cache identity includes the definition and all arguments. Include result-changing inputs such as tenant, locale, filter, or page. Different query functions remain separate even when their arguments and return types match.

## Provide a client

Create the client once in a stable provider component's `create` method:

```rust
use lurq::query::QueryClient;

fn create(ctx: &mut Ctx) -> Self {
  ctx.provide(QueryClient::new());
  Self
}
```

Descendants use the nearest provided client. A nested provider can isolate a subtree. Cloning a client shares its cache; creating a new client creates an isolated cache. Replacing a provider's client rebinds its query observers.

## Choose which trees share data

`QueryClient` already uses `Arc` internally. Pass clones to trees that should share data; use `new()` for an independent cache:

```rust
let queries = QueryClient::new();

// Root accepts QueryClient as its Props.
first_tree.mount_root::<Root>(&mut app, queries.clone());
second_tree.mount_root::<Root>(&mut app, queries.clone());

// This tree has its own cache and invalidation scope.
isolated_tree.mount_root::<Root>(&mut app, QueryClient::new());
```

In `Root`, declare `type Props = QueryClient` and provide that instance in `create`:

```rust
fn create(ctx: &mut Ctx) -> Self {
  ctx.provide(ctx.props::<QueryClient>().clone());
  Self
}
```

Trees receiving clones share cached results, running requests, and invalidation. Trees receiving different clients remain independent. No additional `Arc<QueryClient>` wrapper is needed.

Each attached tree receives a wakeup when shared work becomes ready, and delivers reactive notifications during its own tick, on its own thread. The client serializes shared request polling so concurrent trees cannot start duplicate requests.

Closing one tree removes its observers and runtime registration. Other trees keep their cache and pending requests. When the last tree closes, pending work is cancelled because no UI driver remains. Successful data stays cached while application code retains the client; retention is checked before reusing entries when a tree attaches again. Dropping the last client reference releases the cache.

## Observe during render

```rust
let user = ctx.query(get_user(user_id));

let label = if let Some(user) = user.data() {
  user.name.clone()
} else if let Some(error) = user.error() {
  error.to_string()
} else {
  "Loading...".to_owned()
};

Text::new(&label)
```

`ctx.query` must be called during render. Lurq retains the observer at its query call position. Rerendering with the same descriptor reuses it; changing arguments switches entries. Removing a query call or unmounting releases that observer.

| Handle method | Result |
| --- | --- |
| `data()` | `Option<Arc<T>>`, the last successful value for this key. |
| `error()` | `Option<Arc<E>>`, the latest failed attempt's error. |
| `loading()` | A request is scheduled or running and there is no cached data. |
| `fetching()` | A request is scheduled or running, including a background refresh. |
| `invalidate()` | Mark this entry stale and refresh if it has mounted observers. |
| `refresh()` | Request a fetch even if the entry is fresh or inactive. |

The four read methods participate in normal reactive render tracking. A refresh retains the previous successful data for the same key. A failed refresh also retains that data and exposes the error. Starting another attempt clears the error. Changing keys exposes the new key's cache entry, rather than displaying the old key's value as the new result.

Cloned handles do not add mounted observers. After an entry is evicted, saved handles are empty and their commands are no-ops; observe the descriptor again to obtain the new entry.

## Invalidate after changes

Use the same descriptor for an exact entry, or the generated `all()` selector for every cached argument combination:

```rust
let queries = ctx.query_client();

queries.invalidate(get_user(user_id));
queries.invalidate(get_user::all());
```

For a paginated query, `list_users::all()` includes all cached filters and pages of `list_users`. It does not invalidate a separate `get_user` definition.

An observed entry refreshes in the background. An inactive entry becomes stale and refreshes when observed again. Missing entries are not created. Existing data remains available while a refresh runs.

Invalidation overrides freshness and makes an older in-flight result obsolete. Repeated invalidations before the next tick coalesce into one replacement request. `refresh()` joins an already running current request. These methods enqueue work and return immediately; the runtime applies commands and publishes results during `Tree::tick_futures()`.

For writes, use the existing action API and invalidate the affected reads after success:

```rust
let queries = ctx.query_client();

let save = ctx.future_action(move |changes: UserChanges| {
  let queries = queries.clone();
  async move {
    let saved = api::update_user(changes).await?;
    queries.invalidate(get_user(saved.id));
    queries.invalidate(list_users::all());
    Ok::<_, ApiError>(saved)
  }
});

// In an event handler:
save.run(changes);
```

This keeps `future_action`'s existing behavior: another `.run()` restarts the local task. A dedicated mutation API and optimistic updates are separate future work.

## Freshness and retention

By default, a successful result is fresh for 30 seconds. Inactive entries are retained for five minutes after their last observer leaves.

```rust
use std::time::Duration;
use lurq::query::{QueryClient, QueryClientOptions};

let queries = QueryClient::with_options(QueryClientOptions {
  stale_time: Duration::from_secs(60),
  gc_time: Duration::from_secs(600),
});
```

A definition can override either setting:

```rust
#[lurq::query(stale_time = "10s", gc_time = "2m")]
async fn get_status() -> Result<Status, ApiError> {
  api::get_status().await
}
```

Duration annotations accept an integer followed by `ns`, `us`, `ms`, `s`, `m`, or `h`.

Freshness and retention are independent. Observing stale data schedules a refresh, but merely passing the freshness deadline does not start polling. Ordinary rerenders do not repeatedly fetch stale data or retry failures. There are no automatic retries or focus/reconnect refreshes in this version.

When the last observer leaves, an existing valid request may finish and populate the cache during the retention period. Eviction cancels unfinished work and rejects obsolete completions. An explicit refresh of an inactive entry restarts its retention period.

## Runtime and inspection

The winit shell drives query commands, completions, and retention deadlines automatically. Queries wake the event loop when a future becomes ready or another thread enqueues invalidation, so idle queries do not require continuous polling. Custom shells should continue calling `Tree::tick_futures()` as part of their tick loop.

With a Tokio handle configured through `App::with_tokio_handle`, queries run on a registered runtime. A shared client chooses the first available attached Tokio runtime when starting a request; an existing task stays on the runtime where it started. Closing its starting tree does not cancel that task while other trees remain attached. If the runtime itself shuts down, observed requests restart on another available attached runtime. If all registered runtimes have shut down, requests wait for a live runtime without busy polling.

Enable both `query` and `tokio` for services that require Tokio. Clients without a configured Tokio runtime poll futures cooperatively; blocking I/O in a query body would block that polling thread. Cache changes wake all attached trees, and each tree publishes its observer notifications on its own UI tick.

`queries.inspect()` returns `QueryInfo` records containing definition name, observer count, freshness, fetch activity, and whether data or an error is present. Inspection does not expose arguments or payload contents. A dedicated DevTools query panel is not included yet.
