Done: 2026-10-17, PR #23

# System thermal state

## Behavior and decisions

- Read the system's public ProcessInfo thermal state: Nominal, Fair, Serious or Critical.
  Do not derive state from Celsius readings or claim confirmed throttling from this value.
- Use system Foundation/Objective-C interfaces with correct method ABIs and scoped object
  lifetime management, without adding Cargo dependencies.
- Treat unsupported/unknown state as unavailable. Virtual machines remain unavailable because
  the system API can return Nominal on platforms without thermal-state support.
- Cache the state with the successful hardware sample; expose a read-only accessor and a
  top-level `/api/sample` field without changing existing metric structs or legacy outputs.
- Show the macOS-reported state in terminal/browser Cooling summaries and details, preserving
  existing temperature sensors, fan behavior, expansion preferences and small-window access.
- Keep this a separate feature PR, dependent on memory activity only for shared snapshot and
  interface integration. The person approved complete implementation of both features.

## Tasks

- [x] Add safe public system-state sampling, state decoding and failure/VM tests.
- [x] Integrate same-sample library/HTTP access and both interfaces with unavailable fallbacks.
- [x] Run full checks, independent review and a real rootless ProcessInfo comparison.

## Result

Added safe cached ProcessInfo thermal state and compatible HTTP, terminal and browser views,
preserving existing sensor, fan and panel controls. Full checks and independent review
passed; rootless M2 Max sampling matched a separate Foundation probe. Four states and
unavailable/VM/failure paths have regression coverage without temperature inference.
