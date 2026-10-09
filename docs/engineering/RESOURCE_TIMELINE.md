# ResourceTimeline — adapter contract

**SSOT renderer:** `src/components/timeline/ResourceTimeline.tsx`  
**Tokens:** `src/design-system/resource-timeline-tokens.ts` + `resource-timeline.css`  
**Rules:** `.cursor/rules/unified-interface-primitives.mdc`, `.cursor/rules/resource-timeline-horizon-colors.mdc`

## Purpose

Enterprise resource-vs-time visualization (people × horizon, later teams/assets × load). Visibility and selection only on Availability — not WO scheduling.

## Visual ownership

`react-calendar-timeline/style.css` is a **structural reset base** only (layout classes). Product look and theme (headers, grid, rows, weekend bands, items, selection, scrollbars) come from Design System `resource-timeline.css` plus theme vars on `--surface-*` / `--text-*` / `--color-*`. Adapters must not import the library or invent a second Gantt.

## Adapter props (conceptual)

| Prop | Owner |
|------|--------|
| `groups` | Module — one row per resource (`id`, `title`) |
| `items` | Module — intervals with `group`, `start_time`, `end_time`, `title`, semantic `state` |
| `visibleTimeStart` / `visibleTimeEnd` | Module horizon (ms) |
| `onItemSelect` | Module — open detail workspace |
| `onVisibleTimeChange` | Module — sync horizon when user pans/zooms |
| Zoom chrome | Primitive (in-canvas toolbar) |
| Height | Prefer fill parent (`flex-1` + ResizeObserver); optional fixed `height` override |

## State → token

Map `state` to `mf-rt-item--{state}` / `RESOURCE_TIMELINE_STATE_CLASS`. Block types map via `blockTypeToTimelineState`. Do not paint bars with inline hex or `color-mix(..., #000)`.

## Horizon

Presets: `RESOURCE_TIMELINE_HORIZON_PRESETS` (7 / 14 / 28). Default: `RESOURCE_TIMELINE_DEFAULT_HORIZON_DAYS`. Density: `RESOURCE_TIMELINE_SIDEBAR_WIDTH`, `RESOURCE_TIMELINE_LINE_HEIGHT`, `RESOURCE_TIMELINE_ITEM_HEIGHT_RATIO`.

## Forbidden

- Second Gantt/calendar library in feature code
- Hardcoded cell/bar hex outside the token map
- Fake sub-hour precision when the API only provides day aggregates or block wall-clock intervals
