# Paint the threads sidebar with the code editor's background

- Date: 2026-10-01 16:30
- Status: done except M's visual check
- Repos: zedd   Branch: zedd-bugfixes-and-review-features (or a new one, M's call)
- Folders: crates/theme, crates/sidebar
- Files: crates/theme/src/styles/colors.rs, crates/sidebar/src/sidebar.rs, crates/sidebar/src/sidebar_tests.rs
- Subagents: none

## Context and problem

M sees the threads sidebar (the thread list on the left, with its search bar, project headers and footer) in a lighter gray than the code editor. M wants it in the same gray as a code file.

## Goals / Non-goals

Goals: the whole sidebar column is painted with the editor background in every theme; row hover, selection, the fade behind long titles, the sticky project header, and transparent windows keep working.
Non-goals: the Ctrl-Tab thread switcher popup and other surfaces that use `surface_background` (popovers, modals); the agent panel's own background.

## What is true today

- Screenshot (M, 2026-10-01, VSCode Dark Modern): the sidebar is `#303030` everywhere (search bar, project header, rows, footer); the code file is `#1f1f1f`; the agent panel is `#181818` [verified: pixel samples of the screenshot].
- The sidebar paints `surface_background` at its root [verified: sidebar.rs:8058, 8137], passes it to thread and terminal rows as the backdrop for their title fade [verified: sidebar.rs:6362, 6705], and uses it for project headers [verified: sidebar.rs:2432].
- The sticky project header and sticky-variant header use `surface_overlay_background()`, an opaque version that hides rows scrolling under it [verified: sidebar.rs:2430, 3371]; the rule is `background.blend(surface_background)` when either is opaque, else `panel_overlay_background` [verified: theme/src/styles/colors.rs:466-472].
- A code file's editor paints `editor_background` [verified: editor.rs:11496, `EditorMode::Full { .. } => cx.theme().colors().editor_background`].
- Tests already check sidebar hover colors on opaque and transparent surfaces [verified: sidebar_tests.rs `test_sidebar_action_hover_contrasts_with_row`, `set_sidebar_test_surface_alpha`, `sidebar_painted_background_at`].

## Options considered

| | A: sidebar uses `editor_background` (code) | B: theme override in M's settings |
| --- | --- | --- |
| How | One sidebar helper returns the editor background; every sidebar background site uses it; the sticky header gets the opaque form from a theme rule shared with `surface_overlay_background` | `"theme_overrides": { "surface.background": "#1f1f1f" }` |
| Fit | Matches "the gray of the code file" in every theme, light and dark | One theme only; must be redone per theme |
| Blast radius | Sidebar only | Every surface that uses `surface.background` (popovers, modals, scrollbars, other panels) changes too |
| Fails when | A theme with a translucent editor background (handled by the shared overlay rule) | M switches theme |
| Completeness | 9/10 | 5/10 |

RECOMMENDATION: A, because it changes exactly the sidebar, in every theme, and keeps one rule for opaque overlays.

## Decision

A. The sidebar's background comes from one helper, `sidebar_background(colors)`, returning `editor_background`; its sticky overlays come from `sidebar_overlay_background(colors)`. The theme gains `ThemeColors::overlay_background(base)` (the existing blend-or-panel-overlay rule, generalized), and `surface_overlay_background()` becomes `overlay_background(surface_background)`, so there is one rule for both.

## Design

```
theme colors ── overlay_background(base) ◄── surface_overlay_background()   (unchanged result)
                       ▲
sidebar ── sidebar_background()  = editor_background      <-- changed (was surface_background)
       └── sidebar_overlay_background() = overlay_background(editor_background)   <-- changed
           used by: root, row title fades, project header, sticky header
```

Hover and active row colors already blend onto whatever is behind them, so they follow the new background without changes.

## Risks and mitigations

| Risk | Likelihood | Impact | Mitigation | Noticed by |
| --- | --- | --- | --- | --- |
| A row's title fade still uses the old gray | Medium | Visible band at the end of long titles | All backdrop sites go through the helper; grep for `surface_background` in the sidebar is empty | Grep in ship check; visual check |
| Sticky header turns see-through on a translucent theme | Low | Rows show through the header | Same overlay rule as today, applied to the new base | Existing transparent-surface test, plus the new test |
| Low contrast between sidebar and agent panel | Low | Panels blur together | The sidebar keeps its border; `#1f1f1f` vs `#181818` still differs | M's visual check |

## Acceptance criteria

- Given an opaque theme, when the sidebar renders, then an empty spot of the thread list is painted with `editor_background`.
- Given the sticky project header over scrolled rows, then it is opaque and painted with the editor background.
- Given a translucent surface or editor background, then hover colors still contrast with rows (existing test passes).
- Given any theme, then `surface_overlay_background()` returns what it returned before.

## Rollout and rollback

Ships in the next zedd build. Rollback: revert the commit; no settings or data change.

## Parts

### Part 1: one overlay rule in the theme
- [x] Confirm the editor paints `editor_background` (done when: the editor element's background source is found and cited). editor.rs:11496.
- [x] `ThemeColors::overlay_background(base)`; `surface_overlay_background()` calls it (done when: the existing `surface_overlay_background` test passes unchanged).

### Part 2: sidebar on the editor background
- [x] `sidebar_background` / `sidebar_overlay_background` helpers; root, row fades, project header and sticky header use them (done when: no `surface_background` or `surface_overlay_background` remains in sidebar.rs).

### Part 3: tests and verification
- [x] Test: an empty spot of the thread list paints `editor_background` (done when: it passes, and fails with the old color).
- [x] Existing sidebar color tests pass; sidebar, theme suites; clippy; rustfmt (done when: all pass).
- [ ] Debug build for M's visual check (done when: M confirms).

## Standardized review

- **Bottom line:** executable; no blocking or must-verify findings.
- Goal fidelity: pass (whole sidebar column, every theme; non-goals named). Current-system accuracy: pass (every claim cited, including the editor token at editor.rs:11496). Scope and cohesion: pass (sidebar plus one theme helper). Architecture and dependency direction: pass (the overlay rule lives in the theme, the sidebar decides its own base). Sequencing: pass (theme rule before its users). Completeness: pass (root, fades, headers, sticky header, transparency). Failure and recovery: pass (translucent themes via the shared rule; revert is clean). Verification: pass (new painted-color test that fails on the old color; existing transparency tests). Feasibility: pass. Decision readiness: one choice for M: same branch or a new one.

## Changes made

- `ThemeColors::overlay_background(base)` holds the opaque-overlay rule; `surface_overlay_background()` calls it (its test unchanged and passing). The sidebar's root, row title fades, project header and sticky header use `sidebar_background` (`editor_background`) and `sidebar_overlay_background`; no surface color remains in sidebar.rs. The transparency test helper now varies the sidebar's actual base (`set_sidebar_test_background_alpha`, editor background). New test `test_sidebar_is_painted_like_a_code_file` (fails with the old color). sidebar 159, theme, ui, agent_ui pass; clippy and rustfmt clean.

## Open questions

- Resolved: same branch (M).
