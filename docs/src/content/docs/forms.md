---
title: Forms
description: Form handling, field binding, submission, and the Button component.
---

# Forms

Requires the `form` feature flag.

```toml
lurq = { version = "0.53.0", features = ["form"] }
```

Compound form controls read their defaults from `theme.form()`. See [Theme](../theme/#form-theme) for the strict form field, input, checkbox, slider, and button roles.

## Creating A Form

Create a `FormHandle` during `Component::create` using `ctx.form`. The handle owns field signals and a submit callback.

```rust
use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{FormHandle, FormOptions},
  node::Element,
};

struct Settings {
  form: FormHandle,
}

impl Component for Settings {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let form = ctx
      .form(FormOptions::new().field("user", "Ada").field("email", "ada@example.com"))
      .on_submit(|values| {
        println!("user: {}", values.get_string("user").unwrap_or_default());
        println!("email: {}", values.get_string("email").unwrap_or_default());
      });
    Self { form }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Element::new()
  }
}
```

`FormOptions` supports a builder pattern for default values:

```rust
FormOptions::new()
  .field("user", "Ada Lovelace")
  .field("email", "ada@example.com")
  .field("active", true)
```

Or pass all defaults at once:

```rust
FormOptions::new().default(FormValues::new().with("user", "Ada").with("volume", 50))
```

## Field Signals

`FormHandle` exposes typed signals for each named field. Calling a field accessor creates the signal on first access and returns the same signal on subsequent calls.

```rust
let name: Signal<String> = form.string("user");
let volume: Signal<f64> = form.number("volume");
let active: Signal<bool> = form.bool("active");
```

Bind these signals to input components:

```rust
use lurq::components::{Column, TextInput, Checkbox, FormSliderInput, FormSliderInputProps};

fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
  ctx.form_view(self.form.clone(), |ctx| {
    Column::new()
      .child(TextInput::new(self.form.string("user")).name("user"))
      .child(TextInput::new(self.form.string("email")).name("email"))
      .child(Checkbox::new(self.form.bool("notifications")).name("notifications"))
      .child(ctx.mount::<FormSliderInput>(
        FormSliderInputProps::new(self.form.number_control("volume")).range(0, 100),
      ))
  })
}
```

Give each input a `.name(...)` so the form can collect its value on submission. Inputs without a name are ignored during submit.

`form.number(...)` returns `Signal<f64>`. Use `FormSliderInput` to bridge a numeric form field to an integer slider; the plain `Slider::new` expects `Signal<i32>`, while `Slider::new_f32` expects `Signal<f32>`.

Here is the same pattern as a complete settings form with validation and inline errors:

```rust
use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{validators, Button, Column, FormHandle, FormOptions, Text, TextInput},
  node::Element,
};

struct SettingsForm {
  form: FormHandle,
}

impl Component for SettingsForm {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let form = ctx
      .form(
        FormOptions::new()
          .field("display_name", "")
          .field("email", "")
          .validate_string("display_name", validators::required("Display name is required"))
          .validate_string("email", validators::required("Email is required"))
          .validate_string("email", validators::email("Enter a valid email")),
      )
      .on_submit(|values| {
        println!("saving {}", values.get_string("email").unwrap_or_default());
      });

    Self { form }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let display_name = self.form.string_field("display_name");
    let email = self.form.string_field("email");
    let display_name_error = display_name.error().get();
    let email_error = email.error().get();

    ctx.form_view(self.form.clone(), |_ctx| {
      Column::new()
        .child(Text::new("Display name"))
        .child(TextInput::new(display_name.value()).name(display_name.name()))
        .child(error_text(display_name_error))
        .child(Text::new("Email"))
        .child(TextInput::new(email.value()).name(email.name()).single_line())
        .child(error_text(email_error))
        .child(Button::new("Save settings").submit())
    })
  }
}

fn error_text(error: Option<std::sync::Arc<str>>) -> Element {
  match error {
    Some(error) => Text::new(&error).into(),
    None => Text::new("").into(),
  }
}
```

## Form Component

`ctx.form_view(...)` wraps children in a logical form boundary and provides the current form to reusable controls rendered inside the closure:

```rust
ctx.form_view(form.clone(), |ctx| {
  Column::new()
    .child(ctx.mount::<EmailInput>(EmailInputProps {
      name: "email".into(),
      label: "Email".into(),
    }))
    .child(Button::new("Save").submit())
})
```

Use `ctx.form_view_with(FormProps::new(form.clone()).submit_action(action), |ctx| { ... })` when you need custom form props such as async submit handling. `FormProps::on_submit_data(...)` is available for lower-level custom submit plumbing.

`Form::mount(...)` and `Form::element(...)` remain available for raw children or static trees. A `Form` accepts exactly one child. Wrap multiple children in `Column`, `Row`, or `Stack`.

## Form Context Controls

Reusable form input components can accept only a field name and resolve all field data from the nearest form scope.

```rust
use std::sync::Arc;

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Text, TextInput},
  node::Element,
};

#[derive(Clone, Debug, PartialEq, lurq::DevtoolsInspectable)]
struct EmailInputProps {
  name: Arc<str>,
  label: Arc<str>,
}

struct EmailInput;

impl Component for EmailInput {
  type Props = EmailInputProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let control = ctx.string_control(props.name.clone());
    let error = control.visible_error();

    Column::new()
      .child(Text::new(&props.label))
      .child(
        TextInput::new(control.value())
          .name(control.name())
          .on_blur(control.on_blur()),
      )
      .child(match error {
        Some(error) => Text::new(&error),
        None => Text::new(""),
      })
  }
}
```

Parent components mount those reusable inputs inside `ctx.form_view(...)`:

```rust
use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{validators, Button, Column, FormHandle, FormOptions},
  node::Element,
};

struct AccountForm {
  form: FormHandle,
}

impl Component for AccountForm {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let form = ctx
      .form(
        FormOptions::new()
          .field("email", "")
          .field("password", "")
          .validate_string("email", validators::required("Email is required"))
          .validate_string("email", validators::email("Enter a valid email"))
          .validate_string("password", validators::min_len(8, "Use at least 8 characters")),
      )
      .on_submit(|values| {
        println!("login {}", values.get_string("email").unwrap_or_default());
      });

    Self { form }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    ctx.form_view(self.form.clone(), |ctx| {
      Column::new()
        .child(ctx.mount::<EmailInput>(EmailInputProps {
          name: "email".into(),
          label: "Email".into(),
        }))
        .child(ctx.mount::<EmailInput>(EmailInputProps {
          name: "password".into(),
          label: "Password".into(),
        }))
        .child(Button::new("Sign in").submit())
    })
  }
}
```

Available resolvers:

| Method | Purpose |
| --- | --- |
| `ctx.string_control(name)` | Resolve a string field control from the nearest form. |
| `ctx.number_control(name)` | Resolve a numeric field control from the nearest form. |
| `ctx.bool_control(name)` | Resolve a boolean field control from the nearest form. |
| `ctx.form_control(&control)` | Resolve an explicit `Control<T>` descriptor. |

Resolved controls expose `name`, `value`, `error`, `touched`, `dirty`, `submit_attempted`, `submitting`, `visible_error`, `should_show_error`, `mark_touched`, `validate`, `reset`, and `on_blur`.

For advanced composition outside form context, create explicit descriptors from a form handle:

```rust
let email = form.string_control("email");
let age = form.number_control("age");
let active = form.bool_control("active");
```

## Keyboard Navigation

A form's text inputs, checkboxes, sliders, selects, and buttons are Tab stops without a `tab_index`, in tree order after any positive tab indices; `tab_index(-1)` skips one. A form does not trap Tab: its controls are part of the window's order (or the open modal's), so Tab from the last control moves on to the next stop after the form and Shift+Tab from the first goes back before it. Only a modal traps. See [Focus And Keyboard Navigation](../focus-navigation/).

## Submission

Forms submit when:

- A focused single-line text input receives `Enter` (multiline inputs insert a newline).
- A `Button::submit()` inside the form is clicked.

```rust
use lurq::components::Button;

Column::new()
  .child(TextInput::new(form.string("user")).name("user"))
  .child(Button::new("Save").submit())
```

On submit the form collects all named input values, updates the corresponding field signals, validates the resulting values, and calls the `on_submit` callback with a `FormValues` snapshot when validation passes.

For custom submit plumbing, `FormProps::on_submit_data(...)` receives raw `FormData` from the runtime.

## Async Submit

Use `ctx.future_action` with `FormProps::submit_action(...)` when a form should submit to an async operation. The form validates first, sets `submitting` while the action is pending, blocks duplicate submits, and maps rejected `FormErrors` back into field errors.

```rust
use lurq::components::{
  validators, Button, Column, FormErrors, FormOptions, FormProps, FormValues, Text, TextInput,
};

fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
  let submit = ctx.future_action(|values: FormValues| async move {
    save_settings(values)
      .await
      .map_err(|server_errors| {
        FormErrors::new()
          .with("email", server_errors.email)
          .with_messages("password", server_errors.password)
      })
  });

  let submitting = self.form.submitting().get();
  let email = self.form.string_field("email");
  let email_error = email.error().get();

  ctx.form_view_with(
    FormProps::new(self.form.clone()).submit_action(submit),
    |_ctx| {
      Column::new()
        .child(TextInput::new(email.value()).name(email.name()))
        .child(if let Some(error) = email_error {
          Text::new(&error)
        } else {
          Text::new("")
        })
        .child(Button::new(if submitting { "Saving..." } else { "Save" }).submit())
    },
  )
}
```

The same async submit path works with reusable controls. The button can subscribe to `form.submitting()` while the inputs subscribe only to their own field state:

```rust
use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{
    validators, Button, Column, FormErrors, FormHandle, FormOptions, FormProps, FormValues,
  },
  node::Element,
};

struct SignupForm {
  form: FormHandle,
}

impl Component for SignupForm {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let form = ctx.form(
      FormOptions::new()
        .field("email", "")
        .field("password", "")
        .validate_string("email", validators::required("Email is required"))
        .validate_string("password", validators::min_len(8, "Use at least 8 characters")),
    );

    Self { form }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let submit = ctx.future_action(|values: FormValues| async move {
      create_account(values).await.map_err(|message| {
        FormErrors::new().with("email", message)
      })
    });
    let submitting = self.form.submitting().get();

    ctx.form_view_with(
      FormProps::new(self.form.clone()).submit_action(submit),
      |ctx| {
        Column::new()
          .child(ctx.mount::<EmailInput>(EmailInputProps {
            name: "email".into(),
            label: "Email".into(),
          }))
          .child(ctx.mount::<EmailInput>(EmailInputProps {
            name: "password".into(),
            label: "Password".into(),
          }))
          .child(Button::new(if submitting { "Creating..." } else { "Create account" }).submit())
      },
    )
  }
}
```

If you need to run custom logic around submission, call `form.submit_with(data, |values| { ... })`. It performs the same validation and duplicate-submit guard, sets `submitting` to true, and leaves completion to your callback. Call `form.finish_submit()` or `form.set_submitting(false)` when the work completes.

## Validation

Add validators to `FormOptions`. Validators run in the order they are registered. If any validator fails, `on_submit` is skipped and the optional `on_invalid` callback receives a `FormErrors` snapshot.

```rust
use lurq::components::{validators, FormOptions};

let form = ctx
  .form(
    FormOptions::new()
      .field("email", "")
      .validate_string("email", validators::required("Email is required"))
      .validate_string("email", validators::email("Enter a valid email")),
  )
  .on_submit(|values| {
    println!("email: {}", values.get_string("email").unwrap_or_default());
  })
  .on_invalid(|errors| {
    println!("email error: {}", errors.first("email").unwrap_or_default());
  });
```

Use field error signals to render inline errors:

```rust
let email_error = form.error("email"); // Signal<Option<Arc<str>>>
```

Custom validators can inspect one field and the full form value snapshot:

```rust
use lurq::components::{FormOptions, ValidationResult};

FormOptions::new()
  .validate_string("confirm", |confirm, values| {
    if values.get_string("password") == Some(confirm) {
      ValidationResult::valid()
    } else {
      ValidationResult::invalid("Passwords must match")
    }
  })
```

Built-in validators:

| Validator | Use with |
| --- | --- |
| `validators::required(message)` | `validate_string` |
| `validators::email(message)` | `validate_string` |
| `validators::min_len(min, message)` | `validate_string` |
| `validators::max_len(max, message)` | `validate_string` |
| `validators::range(min, max, message)` | `validate_number` |

Validation and error APIs:

| Method | Purpose |
| --- | --- |
| `form.validate()` | Validate all fields and update error signals. |
| `form.validate_field(name)` | Validate only one field and preserve other errors. |
| `form.error(name)` | Return a `Signal<Option<Arc<str>>>` for the first field error. |
| `form.errors()` | Return the current `FormErrors` snapshot. |
| `form.set_error(name, message)` | Set a manual field error, useful for server responses. |
| `form.set_errors(errors)` | Replace the full error snapshot, useful for server responses. |
| `form.set_field_errors(name, messages)` | Set multiple messages for one field while preserving other fields. |
| `form.clear_error(name)` / `form.clear_errors()` | Clear manual or validation errors. |
| `form.clear_errors_for(names)` | Clear selected field errors while preserving other fields. |

Server errors can be mapped into a form in one call:

```rust
form.set_errors(
  FormErrors::new()
    .with("email", "Email already exists")
    .with_messages("password", ["Too short", "Must include a number"]),
);
```

## Field State And Reset

Forms track dirty and touched state per field. Dirty compares the current value against the form defaults. Touched is explicit for now; mark a field touched from blur handlers or submit flows.

```rust
let email = form.string("email");
let email_dirty = form.dirty("email"); // Signal<bool>
let email_touched = form.touched("email"); // Signal<bool>

form.mark_touched("email");
form.reset_field("email");
form.reset();
```

Submitting a form marks registered fields as touched. Resetting clears errors, touched state, and dirty state while restoring field signals to their defaults.

| Method | Purpose |
| --- | --- |
| `form.dirty(name)` | Return a `Signal<bool>` that tracks whether one field differs from its default. |
| `form.touched(name)` | Return a `Signal<bool>` for one field's touched state. |
| `form.is_dirty()` / `form.is_field_dirty(name)` | Read current dirty state without creating a signal. |
| `form.is_touched()` / `form.is_field_touched(name)` | Read current touched state without creating a signal. |
| `form.mark_touched(name)` | Mark one field as touched. |
| `form.clear_touched(name)` / `form.clear_all_touched()` | Clear touched state. |
| `form.reset()` | Restore all defaults and clear errors/touched/dirty state. |
| `form.reset_field(name)` | Restore one field and clear that field's error/touched/dirty state. |

## Submit State

Use submit-attempted state to decide when errors should be visible for untouched fields.

```rust
let attempted = form.submit_attempted(); // Signal<bool>
let email = form.string_field("email");

let should_show_email_error = email.touched().get() || attempted.get();
```

Submit sets `submit_attempted` to true. `form.reset()` clears it.

| Method | Purpose |
| --- | --- |
| `form.submit_attempted()` | Return a `Signal<bool>` that becomes true after a submit attempt. |
| `form.has_submit_attempted()` | Read submit-attempted state without creating a signal. |
| `form.clear_submit_attempted()` | Clear submit-attempted state. |
| `form.submitting()` | Return a `Signal<bool>` that tracks an in-flight submit. |
| `form.is_submitting()` | Read submitting state without creating a signal. |
| `form.set_submitting(value)` / `form.finish_submit()` | Manually control submitting state. |
| `form.submit_with(data, callback)` | Validate, guard duplicate submits, set submitting, then run a callback with `FormValues`. |
| `form.submit_action(data, action)` | Validate, guard duplicate submits, run a `FutureAction`, and map rejected `FormErrors`. |
| `form.watch_submit_action(action)` | Keep submitting/error state synchronized with an externally run submit action. |

## Field Handles

Use typed field handles when a component or helper needs the field value and its state together.

```rust
let email = form.string_field("email");

TextInput::new(email.value())
  .name(email.name())
  .on_blur({
    let email = email.clone();
    move || email.mark_touched()
  });

let email_error = email.error();
let email_dirty = email.dirty();
```

| Constructor | Value signal |
| --- | --- |
| `form.string_field(name)` | `Signal<String>` |
| `form.number_field(name)` | `Signal<f64>` |
| `form.bool_field(name)` | `Signal<bool>` |

`FormField` exposes `name`, `value`, `error`, `dirty`, `touched`, `is_dirty`, `is_touched`, `mark_touched`, `clear_touched`, `validate`, and `reset`.

## FormValues

`FormValues` is the typed map received in the `on_submit` callback.

| Method | Returns |
| --- | --- |
| `get(name)` | `Option<&FormValue>` |
| `get_string(name)` | `Option<&str>` |
| `get_number(name)` | `Option<f64>` |
| `get_bool(name)` | `Option<bool>` |
| `entries()` | Iterator over `(&str, &FormValue)` |
| `len()` / `is_empty()` | Field count |

`FormValue` is an enum with `String(Arc<str>)`, `Number(f64)`, and `Bool(bool)` variants. It converts from `&str`, `String`, `bool`, `i32`, and `f64`.

## Button

`Button` is a row-based component that renders a clickable element.

```rust
use lurq::components::Button;

Button::new("Click me")
  .on_click(|_| println!("clicked"))
```

| Method | Purpose |
| --- | --- |
| `Button::new(label)` | Button with a text label. |
| `Button::empty()` | Button with no label. Use `.child(...)` to add content. |
| `.child(element)` | Append a child element. |
| `.with_children(iter)` | Append multiple children. |
| `.submit()` | Mark as a submit button (requires `form` feature). Triggers form submission on click. |
| `.button()` | Mark as a regular button (default). |
| `.kind(ButtonKind)` | Set the button kind explicitly. |
| `.spacing(value)` | Gap between children. |
| `.align_items(Alignment)` | Cross-axis alignment. |
| `.justify(Justify)` | Main-axis justification. |

All standard visual modifiers (`.background(...)`, `.rounded(...)`, `.padding(...)`, `.on_click(...)`, etc.) work on `Button` because it implements the typed component API.
