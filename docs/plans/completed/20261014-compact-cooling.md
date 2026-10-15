Done: 2026-10-14, PR #11

# Compact cooling cards

## Behavior and decisions

- Use 120px graphical fans, with a 100px fallback when space is constrained, for both
  single- and dual-fan systems.
- Bound each card to its image, readable title and RPM label. Keep the right column only
  as wide as the cards and telemetry require, and avoid stretching cards in tall windows.
- Preserve any process list that the compact console can show; fall back to compact RPM
  indicators when graphical fans would displace it.
- Preserve the existing materials, independent animation, real RPM, Network Monitoring,
  configuration and output formats. Update the same personal-fork PR #11 and installed command.

## Tasks

- [x] Tighten image sizes, card bounds and right-column width.
- [x] Verify common sizes, large font cells, process visibility and native image placement.
- [x] Run complete checks and review the change in PR #11.
- [x] Refresh and verify the installed command with the previous binary retained.

## Result

Fan images now use 120px or a 100px fallback, with bounded cards and a narrower right column.
Tall windows no longer stretch the cards, useful process rows are preserved, and power titles
remain readable. Native placement, controls, Network Monitoring and the refreshed installation
were verified with the previous binary retained.
