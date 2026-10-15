Done: 2026-10-14, PR #11

# Graphical fan material fidelity

Refine the graphical fans in the existing personal-fork PR #11 against the accepted cooling-card
prototype. Keep upstream read-only and retain the approved single-PR production-line exception.

## Behavior and decisions

- Match the rounded mechanical housing, four recessed screws, inset cavity, layered rim,
  restrained accent glow and fixed hub highlight/shadow at the existing 180–240 pixel sizes.
- Use seven broad curved blades with distinct dark, brushed-metal and highlight regions;
  preserve the first-blade accent and the stationary housing and hub during rotation.
- Derive all material colors from the terminal palette through the existing theme boundary.
  Compare the prototype and product at the same size, angle and palette before accepting it.
- Cache static materials and geometry and reuse output buffers. Preserve the independent
  60 Hz frame target, manual/focus/visibility gates, real fan identities and RPM, saved sampling
  interval, Network Monitoring, output/config compatibility and dependency versions.
- Refresh the installed command after integrated checks, retaining its predecessor and
  verifying fresh-zsh resolution, help and the checked release binary.

## Tasks

- [x] Render the accepted CSS reference and export the current product raster for comparison.
- [x] Implement cached mechanical layers and the curved-blade material model.
- [x] Review equal-size, equal-palette static and rotating-angle product/reference comparisons.
- [x] Verify focused tests, complete checks, CPU/RSS and native-frame submission cadence.
- [x] Complete reviews and update the existing personal-fork PR #11.
- [x] Refresh and verify the installed command with its previous binary retained.

## Result

Cooling panels now use cached mechanical housings, recessed screws, broad curved metal blades
and fixed hub lighting matched against the accepted prototype at native sizes and angles.
Rendering preserves the terminal palette, per-fan RPM, manual animation and the independent
image clock. Partial image writes close before restoring terminal modes. Integrated checks,
rootless Network Monitoring and the refreshed installed command passed; its predecessor is retained.
