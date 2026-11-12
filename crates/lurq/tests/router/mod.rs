mod pattern {
  mod catch_all;
  mod edge_cases;
  mod param_segments;
  mod priority;
  mod static_segments;
  mod wildcard_segments;
}

mod navigation {
  mod back_forward;
  mod push;
  mod replace;
}

mod outlet {
  mod fallback;
  mod nested_outlets;
  mod renders_matched_route;
}

mod guard {
  mod allow;
  mod deny;
  mod redirect;
}

mod params {
  mod extract;
  mod missing;
  mod parsed;
}

mod query {
  mod reads_params;
  mod route_matching;
}

mod state {
  mod persists_across_history;
  mod push_with_state;
}

mod offstage {
  mod keeps_runtime_state;
}

mod link {
  mod click_navigates;
  #[cfg(feature = "markdown")]
  mod markdown_link;
}

mod rerender {
  mod navigation_triggers_single_pass;
  mod only_matched_route_rerenders;
  mod sibling_unchanged;
}

mod tabs {
  mod outlet;
}

mod stack {
  mod custom_bar;
  mod screens;
}
