use std::{
  any::Any,
  collections::{BTreeMap, HashSet},
  fmt,
  sync::Arc,
};

use parking_lot::Mutex;

use super::{
  Control, FormErrors, FormField, FormOptions, FormValue, FormValues, ValidationResult,
  validation::{FieldValidator, InvalidCallback},
};
use crate::{
  app::ctx::{FutureAction, FutureStatus},
  core::signal::{Signal, SignalValue},
  node::FormData,
};

type SubmitCallback = Arc<dyn Fn(FormValues) + Send + Sync>;

#[derive(Clone)]
pub struct FormHandle {
  inner: Arc<FormHandleInner>,
}

struct FormHandleInner {
  defaults: Mutex<FormValues>,
  values: Mutex<FormValues>,
  strings: Mutex<BTreeMap<Arc<str>, Signal<String>>>,
  numbers: Mutex<BTreeMap<Arc<str>, Signal<f64>>>,
  bools: Mutex<BTreeMap<Arc<str>, Signal<bool>>>,
  validators: Vec<FieldValidator>,
  errors: Mutex<FormErrors>,
  // The same shape as the maps beside it.
  #[allow(clippy::type_complexity)]
  error_signals: Mutex<BTreeMap<Arc<str>, Signal<Option<Arc<str>>>>>,
  touched: Mutex<BTreeMap<Arc<str>, bool>>,
  touched_signals: Mutex<BTreeMap<Arc<str>, Signal<bool>>>,
  dirty_signals: Mutex<BTreeMap<Arc<str>, Signal<bool>>>,
  submit_attempted: Mutex<bool>,
  submit_attempted_signal: Mutex<Option<Signal<bool>>>,
  submitting: Mutex<bool>,
  submitting_signal: Mutex<Option<Signal<bool>>>,
  submit_action_watches: Mutex<HashSet<usize>>,
  on_submit: Mutex<Option<SubmitCallback>>,
  on_invalid: Mutex<Option<InvalidCallback>>,
  watch_handles: Mutex<Vec<Box<dyn Any + Send + Sync>>>,
}

impl fmt::Debug for FormHandle {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_tuple("FormHandle").field(&Arc::as_ptr(&self.inner)).finish()
  }
}

impl PartialEq for FormHandle {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.inner, &other.inner)
  }
}

impl FormHandle {
  pub fn new(options: FormOptions) -> Self {
    Self::from_options(options)
  }

  pub(crate) fn with_dirty(options: FormOptions, _dirty_callback: Arc<dyn Fn() + Send + Sync>) -> Self {
    Self::from_options(options)
  }

  fn from_options(options: FormOptions) -> Self {
    let defaults = options.defaults;
    Self {
      inner: Arc::new(FormHandleInner {
        defaults: Mutex::new(defaults.clone()),
        values: Mutex::new(defaults),
        strings: Mutex::new(BTreeMap::new()),
        numbers: Mutex::new(BTreeMap::new()),
        bools: Mutex::new(BTreeMap::new()),
        validators: options.validators,
        errors: Mutex::new(FormErrors::new()),
        error_signals: Mutex::new(BTreeMap::new()),
        touched: Mutex::new(BTreeMap::new()),
        touched_signals: Mutex::new(BTreeMap::new()),
        dirty_signals: Mutex::new(BTreeMap::new()),
        submit_attempted: Mutex::new(false),
        submit_attempted_signal: Mutex::new(None),
        submitting: Mutex::new(false),
        submitting_signal: Mutex::new(None),
        submit_action_watches: Mutex::new(HashSet::new()),
        on_submit: Mutex::new(None),
        on_invalid: Mutex::new(None),
        watch_handles: Mutex::new(Vec::new()),
      }),
    }
  }

  pub fn on_submit(self, on_submit: impl Fn(FormValues) + Send + Sync + 'static) -> Self {
    *self.inner.on_submit.lock() = Some(Arc::new(on_submit));
    self
  }

  pub fn on_invalid(self, on_invalid: impl Fn(FormErrors) + Send + Sync + 'static) -> Self {
    *self.inner.on_invalid.lock() = Some(Arc::new(on_invalid));
    self
  }

  pub fn string(&self, name: impl Into<Arc<str>>) -> Signal<String> {
    let name = name.into();
    if let Some(signal) = self.inner.strings.lock().get(&name).cloned() {
      return signal;
    }

    let initial = self
      .inner
      .values
      .lock()
      .get(&name)
      .map(FormValue::to_string_value)
      .unwrap_or_default();
    self.ensure_default(&name, FormValue::from(initial.clone()));
    let signal = Signal::new(initial);
    self.watch_field_for_dirty(name.clone(), &signal);
    self.inner.strings.lock().insert(name, signal.clone());
    signal
  }

  pub fn number(&self, name: impl Into<Arc<str>>) -> Signal<f64> {
    let name = name.into();
    if let Some(signal) = self.inner.numbers.lock().get(&name).cloned() {
      return signal;
    }

    let initial = self
      .inner
      .values
      .lock()
      .get(&name)
      .and_then(FormValue::as_number)
      .unwrap_or_default();
    self.ensure_default(&name, FormValue::from(initial));
    let signal = Signal::new(initial);
    self.watch_field_for_dirty(name.clone(), &signal);
    self.inner.numbers.lock().insert(name, signal.clone());
    signal
  }

  pub fn bool(&self, name: impl Into<Arc<str>>) -> Signal<bool> {
    let name = name.into();
    if let Some(signal) = self.inner.bools.lock().get(&name).cloned() {
      return signal;
    }

    let initial = self
      .inner
      .values
      .lock()
      .get(&name)
      .and_then(FormValue::as_bool)
      .unwrap_or_default();
    self.ensure_default(&name, FormValue::from(initial));
    let signal = Signal::new(initial);
    self.watch_field_for_dirty(name.clone(), &signal);
    self.inner.bools.lock().insert(name, signal.clone());
    signal
  }

  pub fn string_field(&self, name: impl Into<Arc<str>>) -> FormField<String> {
    let name = name.into();
    FormField::new(name.clone(), self.string(name.clone()), self.clone())
  }

  pub fn number_field(&self, name: impl Into<Arc<str>>) -> FormField<f64> {
    let name = name.into();
    FormField::new(name.clone(), self.number(name.clone()), self.clone())
  }

  pub fn bool_field(&self, name: impl Into<Arc<str>>) -> FormField<bool> {
    let name = name.into();
    FormField::new(name.clone(), self.bool(name.clone()), self.clone())
  }

  pub fn string_control(&self, name: impl Into<Arc<str>>) -> Control<String> {
    Control::new(self.string_field(name))
  }

  pub fn number_control(&self, name: impl Into<Arc<str>>) -> Control<f64> {
    Control::new(self.number_field(name))
  }

  pub fn bool_control(&self, name: impl Into<Arc<str>>) -> Control<bool> {
    Control::new(self.bool_field(name))
  }

  pub fn values(&self) -> FormValues {
    self.current_values()
  }

  pub fn error(&self, name: impl Into<Arc<str>>) -> Signal<Option<Arc<str>>> {
    let name = name.into();
    if let Some(signal) = self.inner.error_signals.lock().get(&name).cloned() {
      return signal;
    }

    let initial = self.inner.errors.lock().first_cloned(&name);
    let signal = Signal::new(initial);
    self.inner.error_signals.lock().insert(name, signal.clone());
    signal
  }

  pub fn errors(&self) -> FormErrors {
    self.inner.errors.lock().clone()
  }

  pub fn touched(&self, name: impl Into<Arc<str>>) -> Signal<bool> {
    let name = name.into();
    if let Some(signal) = self.inner.touched_signals.lock().get(&name).cloned() {
      return signal;
    }

    let signal = Signal::new(self.is_field_touched(&name));
    self.inner.touched_signals.lock().insert(name, signal.clone());
    signal
  }

  pub fn dirty(&self, name: impl Into<Arc<str>>) -> Signal<bool> {
    let name = name.into();
    if let Some(signal) = self.inner.dirty_signals.lock().get(&name).cloned() {
      return signal;
    }

    let signal = Signal::new(self.is_field_dirty(&name));
    self.inner.dirty_signals.lock().insert(name, signal.clone());
    signal
  }

  pub fn submit_attempted(&self) -> Signal<bool> {
    if let Some(signal) = self.inner.submit_attempted_signal.lock().clone() {
      return signal;
    }

    let signal = Signal::new(self.has_submit_attempted());
    *self.inner.submit_attempted_signal.lock() = Some(signal.clone());
    signal
  }

  pub fn submitting(&self) -> Signal<bool> {
    if let Some(signal) = self.inner.submitting_signal.lock().clone() {
      return signal;
    }

    let signal = Signal::new(self.is_submitting());
    *self.inner.submitting_signal.lock() = Some(signal.clone());
    signal
  }

  pub fn is_touched(&self) -> bool {
    self.inner.touched.lock().values().any(|touched| *touched)
  }

  pub fn has_submit_attempted(&self) -> bool {
    *self.inner.submit_attempted.lock()
  }

  pub fn is_submitting(&self) -> bool {
    *self.inner.submitting.lock()
  }

  pub fn is_field_touched(&self, name: &str) -> bool {
    self.inner.touched.lock().get(name).copied().unwrap_or(false)
  }

  pub fn is_dirty(&self) -> bool {
    let values = self.current_values();
    let defaults = self.inner.defaults.lock().clone();

    values.entries().any(|(name, value)| defaults.get(name) != Some(value))
      || defaults.entries().any(|(name, value)| values.get(name) != Some(value))
  }

  pub fn is_field_dirty(&self, name: &str) -> bool {
    let values = self.current_values();
    let defaults = self.inner.defaults.lock();
    values.get(name) != defaults.get(name)
  }

  pub fn mark_touched(&self, name: impl Into<Arc<str>>) {
    self.set_touched(name.into(), true);
  }

  pub fn clear_touched(&self, name: &str) {
    let mut touched = self.inner.touched.lock();
    touched.remove(name);
    drop(touched);
    self.update_touched_signal(name, false);
  }

  pub fn clear_all_touched(&self) {
    self.inner.touched.lock().clear();
    let signals = self
      .inner
      .touched_signals
      .lock()
      .iter()
      .map(|(name, signal)| (name.clone(), signal.clone()))
      .collect::<Vec<_>>();

    for (_, signal) in signals {
      signal.set(false);
    }
  }

  pub fn clear_submit_attempted(&self) {
    self.set_submit_attempted(false);
  }

  pub fn set_submitting(&self, submitting: bool) {
    self.set_submitting_state(submitting);
  }

  pub fn finish_submit(&self) {
    self.set_submitting_state(false);
  }

  pub fn set_error(&self, name: impl Into<Arc<str>>, message: impl Into<Arc<str>>) {
    let name = name.into();
    let mut errors = self.inner.errors.lock().clone();
    errors.set_one(name, message.into());
    self.replace_errors(errors);
  }

  pub fn set_errors(&self, errors: FormErrors) {
    self.replace_errors(errors);
  }

  pub fn set_field_errors(&self, name: impl Into<Arc<str>>, messages: impl IntoIterator<Item = impl Into<Arc<str>>>) {
    let mut errors = self.inner.errors.lock().clone();
    errors.set_messages(name, messages);
    self.replace_errors(errors);
  }

  pub fn clear_error(&self, name: &str) {
    let mut errors = self.inner.errors.lock().clone();
    errors.remove(name);
    self.replace_errors(errors);
  }

  pub fn clear_errors_for<'a>(&self, names: impl IntoIterator<Item = &'a str>) {
    let mut errors = self.inner.errors.lock().clone();
    for name in names {
      errors.remove(name);
    }
    self.replace_errors(errors);
  }

  pub fn clear_errors(&self) {
    self.replace_errors(FormErrors::new());
  }

  pub fn validate(&self) -> bool {
    let values = self.current_values();
    self.validate_values(&values).is_empty()
  }

  pub fn validate_field(&self, name: &str) -> bool {
    let values = self.current_values();
    let mut errors = self.inner.errors.lock().clone();
    errors.remove(name);

    for validator in self
      .inner
      .validators
      .iter()
      .filter(|validator| validator.name.as_ref() == name)
    {
      if let ValidationResult::Invalid(message) = (validator.validate)(values.get(name), &values) {
        errors.push(validator.name.clone(), message);
      }
    }

    let is_valid = errors.get(name).is_none();
    self.replace_errors(errors);
    is_valid
  }

  pub fn submit(&self, data: FormData) {
    let Some(values) = self.prepare_submit(data) else {
      return;
    };

    let on_submit = self.inner.on_submit.lock().clone();
    if let Some(on_submit) = on_submit {
      on_submit(values);
    }
  }

  pub fn submit_with(&self, data: FormData, submit: impl FnOnce(FormValues)) -> bool {
    if self.is_submitting() {
      return false;
    }

    let Some(values) = self.prepare_submit(data) else {
      return false;
    };

    self.set_submitting_state(true);
    submit(values);
    true
  }

  pub fn submit_action<T>(&self, data: FormData, action: &FutureAction<FormValues, T, FormErrors>) -> bool
  where
    T: SignalValue + Clone + PartialEq + Send + Sync + 'static,
  {
    self.watch_submit_action(action);
    self.submit_with(data, |values| action.run(values))
  }

  pub fn watch_submit_action<T>(&self, action: &FutureAction<FormValues, T, FormErrors>)
  where
    T: SignalValue + Clone + PartialEq + Send + Sync + 'static,
  {
    let state = action.state();
    let id = state.id();
    if !self.inner.submit_action_watches.lock().insert(id) {
      return;
    }

    let form = self.clone();
    let state_for_watch = state.clone();
    let handle = state.watch(move || {
      let state = state_for_watch.get_untracked();
      match state.status {
        FutureStatus::Pending => form.set_submitting_state(true),
        FutureStatus::Rejected => {
          form.set_submitting_state(false);
          if let Some(errors) = state.error {
            form.set_errors(errors);
          }
        }
        FutureStatus::Idle | FutureStatus::Fulfilled => form.set_submitting_state(false),
      }
    });
    self.inner.watch_handles.lock().push(Box::new(handle));
  }

  fn prepare_submit(&self, data: FormData) -> Option<FormValues> {
    let values = self.values_from_submit(data);
    *self.inner.values.lock() = values.clone();
    self.set_submit_attempted(true);
    self.mark_registered_fields_touched();
    self.refresh_all_dirty_signals();

    let errors = self.validate_values(&values);
    if !errors.is_empty() {
      let on_invalid = self.inner.on_invalid.lock().clone();
      if let Some(on_invalid) = on_invalid {
        on_invalid(errors);
      }
      return None;
    }

    Some(values)
  }

  fn values_from_submit(&self, data: FormData) -> FormValues {
    let strings = self
      .inner
      .strings
      .lock()
      .iter()
      .map(|(name, signal)| (name.clone(), signal.clone()))
      .collect::<Vec<_>>();
    let numbers = self
      .inner
      .numbers
      .lock()
      .iter()
      .map(|(name, signal)| (name.clone(), signal.clone()))
      .collect::<Vec<_>>();
    let bools = self
      .inner
      .bools
      .lock()
      .iter()
      .map(|(name, signal)| (name.clone(), signal.clone()))
      .collect::<Vec<_>>();

    let mut values = self.current_values();
    for (name, value) in data.entries() {
      values.set(name.as_str(), value.to_owned());
    }

    for (name, signal) in strings {
      if let Some(value) = data.get(&name) {
        let value = value.to_owned();
        signal.set(value.clone());
        values.set(name, value);
      }
    }

    for (name, signal) in numbers {
      if let Some(value) = data.get(&name).and_then(|value| value.parse::<f64>().ok()) {
        signal.set(value);
        values.set(name, value);
      }
    }

    for (name, signal) in bools {
      let value = data.get(&name).is_some();
      signal.set(value);
      values.set(name, value);
    }

    values
  }

  pub fn reset(&self) {
    let defaults = self.inner.defaults.lock().clone();
    *self.inner.values.lock() = defaults.clone();

    let strings = self
      .inner
      .strings
      .lock()
      .iter()
      .map(|(name, signal)| (name.clone(), signal.clone()))
      .collect::<Vec<_>>();
    let numbers = self
      .inner
      .numbers
      .lock()
      .iter()
      .map(|(name, signal)| (name.clone(), signal.clone()))
      .collect::<Vec<_>>();
    let bools = self
      .inner
      .bools
      .lock()
      .iter()
      .map(|(name, signal)| (name.clone(), signal.clone()))
      .collect::<Vec<_>>();

    for (name, signal) in strings {
      signal.set(defaults.get(&name).map(FormValue::to_string_value).unwrap_or_default());
    }
    for (name, signal) in numbers {
      signal.set(defaults.get(&name).and_then(FormValue::as_number).unwrap_or_default());
    }
    for (name, signal) in bools {
      signal.set(defaults.get(&name).and_then(FormValue::as_bool).unwrap_or_default());
    }

    self.clear_errors();
    self.clear_all_touched();
    self.clear_submit_attempted();
    self.finish_submit();
    self.refresh_all_dirty_signals();
  }

  pub fn reset_field(&self, name: &str) {
    let default = self.inner.defaults.lock().get(name).cloned();
    let mut values = self.inner.values.lock().clone();
    match default.clone() {
      Some(value) => values.set(name, value),
      None => values.remove(name),
    }
    *self.inner.values.lock() = values;

    let string_signal = self.inner.strings.lock().get(name).cloned();
    let number_signal = self.inner.numbers.lock().get(name).cloned();
    let bool_signal = self.inner.bools.lock().get(name).cloned();

    if let Some(signal) = string_signal {
      signal.set(default.as_ref().map(FormValue::to_string_value).unwrap_or_default());
    }
    if let Some(signal) = number_signal {
      signal.set(default.as_ref().and_then(FormValue::as_number).unwrap_or_default());
    }
    if let Some(signal) = bool_signal {
      signal.set(default.as_ref().and_then(FormValue::as_bool).unwrap_or_default());
    }

    self.clear_error(name);
    self.clear_touched(name);
    self.refresh_dirty_signal(name);
  }

  fn current_values(&self) -> FormValues {
    let mut values = self.inner.values.lock().clone();

    for (name, signal) in self.inner.strings.lock().iter() {
      values.set(name.clone(), signal.get_untracked());
    }
    for (name, signal) in self.inner.numbers.lock().iter() {
      values.set(name.clone(), signal.get_untracked());
    }
    for (name, signal) in self.inner.bools.lock().iter() {
      values.set(name.clone(), signal.get_untracked());
    }

    values
  }

  fn validate_values(&self, values: &FormValues) -> FormErrors {
    let mut errors = FormErrors::new();

    for validator in &self.inner.validators {
      if let ValidationResult::Invalid(message) = (validator.validate)(values.get(&validator.name), values) {
        errors.push(validator.name.clone(), message);
      }
    }

    self.replace_errors(errors.clone());
    errors
  }

  fn replace_errors(&self, errors: FormErrors) {
    *self.inner.errors.lock() = errors.clone();
    let signals = self
      .inner
      .error_signals
      .lock()
      .iter()
      .map(|(name, signal)| (name.clone(), signal.clone()))
      .collect::<Vec<_>>();

    for (name, signal) in signals {
      signal.set(errors.first_cloned(&name));
    }
  }

  fn ensure_default(&self, name: &Arc<str>, value: FormValue) {
    let mut defaults = self.inner.defaults.lock();
    if defaults.contains(name) {
      return;
    }

    defaults.set(name.clone(), value.clone());
    let mut values = self.inner.values.lock();
    if !values.contains(name) {
      values.set(name.clone(), value);
    }
  }

  fn set_touched(&self, name: Arc<str>, is_touched: bool) {
    if is_touched {
      self.inner.touched.lock().insert(name.clone(), true);
    } else {
      self.inner.touched.lock().remove(&name);
    }
    self.update_touched_signal(&name, is_touched);
  }

  fn update_touched_signal(&self, name: &str, is_touched: bool) {
    if let Some(signal) = self.inner.touched_signals.lock().get(name).cloned() {
      signal.set(is_touched);
    }
  }

  fn set_submit_attempted(&self, attempted: bool) {
    *self.inner.submit_attempted.lock() = attempted;
    if let Some(signal) = self.inner.submit_attempted_signal.lock().clone() {
      signal.set(attempted);
    }
  }

  fn set_submitting_state(&self, submitting: bool) {
    *self.inner.submitting.lock() = submitting;
    if let Some(signal) = self.inner.submitting_signal.lock().clone() {
      signal.set(submitting);
    }
  }

  fn refresh_dirty_signal(&self, name: &str) {
    if let Some(signal) = self.inner.dirty_signals.lock().get(name).cloned() {
      signal.set(self.is_field_dirty(name));
    }
  }

  fn refresh_all_dirty_signals(&self) {
    let names = self.inner.dirty_signals.lock().keys().cloned().collect::<Vec<_>>();
    for name in names {
      self.refresh_dirty_signal(&name);
    }
  }

  fn mark_registered_fields_touched(&self) {
    let mut names = self
      .inner
      .defaults
      .lock()
      .entries()
      .map(|(name, _)| Arc::<str>::from(name))
      .collect::<Vec<_>>();
    names.extend(self.inner.strings.lock().keys().cloned());
    names.extend(self.inner.numbers.lock().keys().cloned());
    names.extend(self.inner.bools.lock().keys().cloned());
    names.extend(self.inner.validators.iter().map(|validator| validator.name.clone()));
    names.sort();
    names.dedup();

    for name in names {
      self.set_touched(name, true);
    }
  }

  fn watch_field_for_dirty<T>(&self, name: Arc<str>, signal: &Signal<T>)
  where
    T: SignalValue + Send + Sync + 'static,
  {
    let form = self.clone();
    let handle = signal.watch(move || {
      form.refresh_dirty_signal(&name);
    });
    self.inner.watch_handles.lock().push(Box::new(handle));
  }
}
