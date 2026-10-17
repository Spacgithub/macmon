# Terminal overview and detail controls

## Behavior and decisions

- Depend on the temperature-details feature and retain terminal ANSI theme colors.
- Default to concise live summaries; independently expand Compute, Memory, Power,
  Network, Cooling/temperature and Processes, with an all-detail/all-summary shortcut.
- Make the controls discoverable in panel titles and help, using consistent keyboard
  navigation without stealing keys while typing filters or using existing overlays.
- Summaries retain named top-three interfaces, top-five processes, current temperatures
  and RPM; details expose all list rows, real temperature sensors and temperature trends.
- Keep existing process sorting/filtering/selection and the `p` sampling switch distinct
  from detail mode. Do not lose view state when merely changing detail level.
- Save additional optional preferences with defaults while preserving all released config
  field names and values. Handle old/invalid config and tiny/resized terminals safely.
- Collapsed Cooling stops graphics/frame writes; restoring details reuses fan identity.
- The person approved the complete three-PR scope and expanded feature budgets.

## Tasks

- [x] Add panel detail preferences and consistent keyboard controls.
- [x] Integrate compact summaries, accessible full lists and temperature details/history.
- [x] Verify geometry combinations, resize, filtering, selection and fan cleanup.
- [ ] Run checks, rootless runtime validation and independent review.
