mod text {
  mod copy;
  mod ligatures;
  mod scrollbar_lane;
  mod selectable;
}

mod clipboard_content;
mod paste;

mod synthetic {
  mod injection;
}

mod select {
  mod interaction;
  mod keyboard;
  mod rerender;
  mod style;
}

mod text_input {
  mod basic_layout;
  mod clipped_paint;
  mod controller;
  mod controller_focus;
  mod copy_selection;
  mod editing;
  mod empty_value;
  mod focus;
  mod ime;
  mod letter_spacing;
  mod line_metrics;
  mod masked_editing;
  mod mode_rerender;
  mod overflow_anchor;
  mod preserves_editing_state_across_render;
  mod renders_caret;
  mod scroll_presentation;
  mod scroll_tokens;
  mod scroll_wheel;
  mod scrollbar_lane;
  mod soft_wrap_selection;
  mod style;
  #[cfg(feature = "markdown")]
  mod styled;
  mod tokens;
  mod typing_updates_value;
  mod vertical_rows;
}

mod checkbox {
  mod double_toggle;
  mod keyboard_toggle;
  mod renders_style;
  mod toggles_on_click;
}

#[cfg(feature = "form")]
mod forms {
  mod async_submit;
  mod compound;
  mod control;
  mod enter_submits;
  mod form_data;
  mod logical_wrapper;
  mod prefill;
  mod rerender;
  mod state;
  mod submit_button;
  mod tab_navigation;
  mod validation;
}

mod slider {
  mod avoids_unchanged_updates;
  mod clamps_min_max;
  mod drag_updates_value;
  mod keyboard_update;
  mod renders_thumb;
  mod suppresses_parent_click;
  mod updates_from_click;
}

mod pointer {
  mod modifiers;
  mod native_window_gesture;
}

mod control_fill;
mod focus_on_press;
mod focus_visible;
mod focused_style;
mod scroll_into_view;
mod tab_order;
