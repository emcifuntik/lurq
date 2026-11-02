mod app_lifetime;
mod click;
mod cursor;
mod double_click;
mod dropdown_switch;
mod dropdown_trigger;
mod drag;
mod element_lookup {
  mod by_class;
  mod by_id;
  mod mutates_rect;
  mod mutation;
  mod synthetic_click;
  mod transient_mutation;
  mod typed_interaction;
}
mod element_ref_interaction;
#[cfg(feature = "form")]
mod focus_reconciliation;
mod futures {
  mod created_in_create;
  mod future_action_restarts_from_state_watch;
  mod future_action_runs_on_run;
  mod future_resolves;
  mod future_restarts_on_deps_change;
  mod outcome;
  mod run_if_idle;
  #[cfg(feature = "tokio")]
  mod tokio_future_action_uses_runtime;
  #[cfg(feature = "tokio")]
  mod tokio_future_uses_runtime;
  #[cfg(feature = "tokio")]
  mod tokio_tasks;
}
mod headless_pass;
mod hidden_overlay;
mod hover_after_scroll;
mod input_source;
// `Tree::set_canvas_waker` stands in for the shell's event-loop waker.
#[cfg(feature = "canvas")]
mod loop_wake {
  mod local_future_wakes_loop;
  mod signal_set_wakes_loop;
  mod timer_start_wakes_loop;
  #[cfg(feature = "tokio")]
  mod tokio_work_wakes_loop;
}
mod history_swipe;
mod modal_resize;
mod mouse_leave;
mod outside_press;
mod overlay;
mod pass_report;
mod perf_overlay;
#[cfg(feature = "image")]
mod render_order;
mod scroll_axis_dominance;
mod scroll_state;
mod scrollbar_hits;
mod stack_depth;
mod text_reflow;
mod text_scale_wrap;
mod text_shadow;
mod timers {
  mod all_due_timers_fire;
  mod interval_repeats_until_stopped;
  mod signal_update_rerenders;
  mod timeout_fires_once;
  mod timeout_restart_replaces_pending_fire;
}
