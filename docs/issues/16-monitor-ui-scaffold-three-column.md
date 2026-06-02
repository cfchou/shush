# Monitor UI scaffold (three-column shell)

Status: ready-for-agent

Priority: high

## What to build

**Frontend only.** Create the monitor page UI scaffold so it matches the three-column design direction in the provided mocks, while keeping data wiring and command behavior in the existing monitor issues.

This slice is about **layout, visual structure, idle state, and scaffold interactions**. It should not re-specify terminal streaming logic or command approval business logic beyond the UI shell needed to host them.

The scaffold **must preserve the current completed terminal feature**: when a real session is selected, the existing live terminal stream continues to render inside the new center-column shell.

**Design requirements from the provided mocks:**
- Three-column app shell:
  - Left session sidebar: `280px`
  - Center content column: `minmax(1024px, 1fr)`
  - Right command sidebar: `360px`
- Collapsible left and right sidebars with header toggle buttons
- White/light chrome around a dark fixed-size terminal surface
- Center stage keeps a `1024x768` terminal frame presentation, even in idle state
- Sticky monitor header with:
  - left-sidebar toggle button
  - right-sidebar toggle button
  - session context block (`badge`, title, caption)
  - YOLO mode toggle treatment
  - stop/abort action button affordance
- Idle monitor state:
  - “No session selected” / placeholder treatment in the center terminal
  - empty right sidebar message when there are no command cards
- Active monitor state scaffold:
  - terminal surface remains centered inside the middle column
  - current live terminal behavior is preserved for selected sessions
  - right sidebar supports stacked expandable command cards
  - hidden-sidebar banners/copy appear in the header when either sidebar is collapsed

**Files to add/update:**
- `frontend/src/monitor/monitor.ts`
  - Rework monitor page rendering around the three-column shell
  - Support both idle and active session-present scaffold states
  - Add left/right sidebar collapse state
  - Render header context, YOLO toggle UI shell, terminal container, and right sidebar container
- `frontend/src/monitor/monitor.css`
  - Introduce the design tokens and layout styles needed for the mock-aligned shell
  - Add responsive behavior so the page still loads and remains usable on narrower widths
  - Preserve a clear separation between app chrome and terminal surface
- `frontend/src/main.ts`
  - Ensure routing can render the idle scaffold when no concrete monitor session is selected, or add the smallest route/state glue needed so the monitor can show an empty placeholder state without breaking existing routing

**Left sidebar scaffold:**
- Section title for active sessions
- Session list item styling matching the mock direction:
  - compact card-like rows
  - active row state
  - status dot treatment
  - primary title + muted meta line
- Back the sidebar list with the existing session-list API from Issue 02
- Clicking a session row navigates to `/monitor/:id`
- No new backend API is required for the sidebar list itself

**Center scaffold:**
- Sticky top header with controls and session context
- Center stage area with padding around the terminal frame
- Terminal frame:
  - rounded corners
  - dark header strip
  - dark body area
  - fixed `1024x768` presentation in both placeholder and live terminal cases
- When a session is selected, render the existing terminal component/stream inside this frame instead of the idle placeholder
- Idle state body with icon/mark, title, and explanatory text from the mock direction

**Right sidebar scaffold:**
- Header title and supporting copy
- Empty state when there are no cards
- Card container styles for the later command-card implementation:
  - stacked list
  - rounded cards
  - hover elevation
  - expandable details section
  - action button row affordances

**Interaction scaffold only:**
- Left/right header toggle buttons collapse their respective sidebars
- Hidden-sidebar helper text appears in the header when a sidebar is collapsed
- YOLO toggle updates its visual on/off state locally
- Stop button can remain a non-destructive placeholder if abort wiring is not yet available in this slice

**Testing expectations:**
- Add frontend unit tests that cover:
  - idle scaffold rendering
  - sidebar collapse behavior
  - empty right-rail rendering
  - YOLO toggle visual state changes
- Add or update a Playwright E2E that verifies the monitor shell renders and both sidebars can be toggled

## Acceptance criteria

- [x] Monitor page uses a three-column shell aligned with the provided mock proportions: left `280px`, center `minmax(1024px, 1fr)`, right `360px`
- [x] The center column presents the terminal inside a fixed `1024x768` framed surface with light outer chrome and dark terminal styling
- [x] For a selected session, the current live terminal feature continues to render inside the new center-column shell
- [x] The monitor header is sticky and includes left/right sidebar toggles, session context, YOLO control styling, and a stop-action affordance
- [x] Left and right sidebars can each be collapsed independently from the header controls
- [x] When a sidebar is collapsed, the header shows the corresponding hidden-sidebar helper text
- [x] The idle monitor state renders a terminal placeholder in the center and an empty command-sidebar message on the right
- [x] The left sidebar renders session list rows in the visual style established by the mock
- [x] The left sidebar is populated from the existing session-list API, and clicking a session row navigates to `/monitor/:id`
- [x] The right sidebar provides the styled scaffold for future command cards, including expandable-card affordances
- [x] Frontend unit tests cover idle render plus sidebar toggle behavior
- [x] Frontend E2E verifies the monitor shell and sidebar toggles

## Blocked by

- Issue 02: Session lifecycle: create, list, dashboard (`docs/issues/02-session-lifecycle-create-list-dashboard.md`) — provides baseline frontend app scaffold and routing

## Related

- Issue 03: FE master connection + terminal stream (`docs/issues/03-fe-master-terminal-stream.md`) — plugs live terminal data into this shell
- Issue 10: Monitor card list (`docs/issues/10-monitor-card-list.md`) — fills the right sidebar with real command cards and actions
- Issue 11: Command submit from browser (`docs/issues/11-command-submit-from-browser.md`) — likely consumes the center/header scaffold once browser-side submission UX is added
- Issue 12: Abort (`docs/issues/12-abort.md`) — wires the stop action to real abort behavior

## References

- Plan: `docs/shush-v01-plan.md` (frontend scaffold, monitor route, monitor CSS)
- Design mock: `/Users/chifeng/Downloads/shush-three-column-idle.html`
- Design mock: `/Users/chifeng/Downloads/shush-three-column-monitor-3.html`
