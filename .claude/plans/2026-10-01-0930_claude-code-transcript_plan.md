# Claude Code style transcript for every agent thread

- Date: 2026-10-01 09:30
- Status: done (debug check pending M)
- Repos: zedd   Branch: zedd-claude-code-transcript (same tree as main f7c5d286)
- Folders: crates/agent_ui, crates/acp_thread, crates/agent_settings, crates/settings_content, crates/settings_ui, assets
- Files: conversation_view/thread_view.rs, conversation_view/tool_run_summary.rs (new), conversation_view/thread_search_bar.rs, entry_view_state.rs, conversation_view.rs (tests), agent_ui.rs (action), agent_settings.rs, settings_content/src/agent.rs, settings_ui page_data.rs, default.json, default-macos.json
- Subagents: none
- Research: .claude/research/2026-10-01-0856_claude-code-transcript-ui_research.md

## Context and problem

Every tool call is its own card or icon row, so a working thread is a stack of boxes (M's screenshot: three gray command boxes in a row). Claude Code desktop folds each run of tool calls into one muted line ("Read 9 files ›") and has Normal / Thinking / Verbose view modes. M wants that look for the native agent and every ACP agent.

## Goals / Non-goals

Goals:
- `agent.transcript_view`: `normal` (default), `thinking`, `verbose`, with a "Transcript view" dropdown next to the send button and ctrl-o (macOS) to cycle.
- Normal and Thinking: each run of consecutive tool calls is one muted summary line with a chevron; open it to see one line per tool; open a tool line to see today's full rendering of that tool.
- Normal hides thinking; Thinking and Verbose show it as today.
- Verbose is exactly today's rendering.
- Works the same for native and ACP agents (built only from fields both send).

Non-goals:
- Composer redesign, user-prompt restyle (already a rounded box), the working indicator (already "spinner · elapsed · tokens"), diff pane, finished-turn collapse ("Worked for 4m").
- Hiding mid-turn assistant text (the desktop app's reported bug #94354); text is never folded.

## What is true today

- One list row per thread entry [verified: thread_view.rs:6218-6248]; an entry can render `Empty` [verified: 6554-6576, canceled empty tool calls].
- Tool call dispatch in `render_entry` [verified: 6550-6598] goes to `render_any_tool_call` [verified: 8448].
- Expansion state per tool call: `EntryViewState::expanded_tool_calls`; content visible if expanded or awaiting permission [verified: entry_view_state.rs:80-100].
- New diff / terminal auto-expands per `expand_edit_card` / `expand_terminal_card` [verified: thread_view.rs:1317-1343].
- Thread search scans tool content only when `is_tool_call_content_visible` [verified: thread_search_bar.rs:373].
- Row heights are cached; `ListState::remeasure_items(range)` re-measures [verified: gpui list.rs:412; used at thread_view.rs:4200].
- `BufferDiff::changed_row_counts()` gives (added, removed) and `MultiBuffer::diff_for(id)` reaches it for every diff shape [verified: buffer_diff.rs:2203, multi_buffer.rs:2322].
- The working indicator already shows spinner, elapsed time, tokens [verified: thread_view.rs:7494-7560].

## Options considered

See the research file: A (view modes + folded lines at render time), B (one line per tool, no folding), C (separate display-row model).

RECOMMENDATION: A because it matches the documented modes and observed folding, and keeps the one-row-per-entry list that search, fork and scroll rely on.

## Decision

Option A, with these calls made on M's behalf:
- Default `normal`.
- Never folded (they end a run): user messages, visible assistant text, visible thinking, elicitations, subagent tool calls, and any tool call awaiting permission. A failed tool stays in its run and the line shows "· N failed" in the error color.
- Folded content is not searched (same rule as today: only open content is searched).
- Auto-expanding new edit / terminal cards happens only in Verbose.

## Design

```
render_entry(ix)
  tool call, mode verbose or not foldable ---> render_any_tool_call (today)
  tool call, foldable, mode normal/thinking
     run = tool_run_range(ix)                       <-- new: walks neighbours
     ix == run.start ---> summary line "Read 9 files ›"   <-- new
     run open ---------> member line "Read a.rs ›"         <-- new
                          member open -> render_any_tool_call (today)
     run closed, ix > start ---> Empty
  assistant message, mode normal ---> thought chunks dropped   <-- changed
```

- Run membership: an entry is `Foldable` (tool call, not subagent, not awaiting permission, not a canceled empty call), `Transparent` (renders nothing in this mode: blank assistant message, thought-only assistant message in normal, canceled empty call), or `Boundary` (everything else). A run is a maximal span of Foldable/Transparent entries, trimmed to its first and last Foldable.
- Run state: `EntryViewState::open_tool_runs: HashSet<ToolCallId>` keyed by the run's first tool call. Closing a run also closes its members, so "content visible" keeps meaning "on screen".
- Pure summary module `tool_run_summary.rs`: `ToolSummaryItem { kind, subject, line_counts, running, failed }` -> summary text, singular member text. Phrases follow the observed wording ("Read 9 files", "Edited globals.css +3 -0", "Searched code, read a file", present tense while running: "Reading package.json").
- Search visibility: one owner, `ThreadView`-independent rule in `EntryViewState::is_tool_call_content_visible` stays; member expansions are cleared when a run closes and on switching away from verbose, so the existing check stays correct.
- Re-measure: toggling a run re-measures its range; changing mode re-measures all rows.
- Settings: content `Option<TranscriptView>`, resolved `TranscriptView`, default.json `"transcript_view": "normal"`, settings UI dropdown; action `agent::CycleTranscriptView` bound to ctrl-o in `AcpThread` (macOS).

## Risks and mitigations

| Risk | Likelihood | Impact | Mitigation | Noticed by |
| --- | --- | --- | --- | --- |
| Existing card-centric tests break under default normal | high | low | Those tests are about verbose rendering: set verbose in their setup | test run |
| ACP agent sends no `kind` | medium | low | Falls back to "Used <title>" wording | M's manual check |
| Permission prompt hidden in a fold | low | high | Awaiting-permission calls are boundaries; test | test |
| Stale row heights after toggling | medium | medium | remeasure run range / all rows | test + manual |
| Run start shifts when an entry before it becomes transparent | low | low | fold state just resets | manual |

## Acceptance criteria

- Given normal mode and three consecutive finished tool calls, when the thread renders, then one summary line shows and no tool cards.
- Given that line, when clicked, then one member line per tool shows; when a member line is clicked, then today's rendering of that tool shows.
- Given a tool call awaiting permission inside a run, then it renders as today's card with its buttons.
- Given verbose mode, then rendering matches today (existing tests pass in verbose).
- Given normal mode, then thinking blocks are hidden; in thinking mode they show.
- Given the dropdown or ctrl-o, then the mode changes and is saved to settings.

## Rollout and rollback

Default normal; any user can set `"transcript_view": "verbose"` to get today's look. Rollback: revert the branch.

## Parts

### Part 1: Setting, action, dropdown
- [x] `TranscriptView` setting end to end (done when: settings suites pass, settings UI shows it)
- [x] `CycleTranscriptView` action + ctrl-o + dropdown next to send (done when: test cycles normal -> thinking -> verbose -> normal)

### Part 2: Summary wording
- [x] `tool_run_summary.rs` with unit tests (done when: wording tests for read/edit/mixed/running/failed/other pass)

### Part 3: Folding in the thread
- [x] run detection, summary line, member lines, open state, re-measure (done when: GPUI tests for fold, open, member open pass)
- [x] permission and subagent boundaries (done when: awaiting-permission test passes)
- [x] thinking per mode, auto-expand only in verbose, search rule (done when: tests pass)
- [x] line counts for edits (done when: single-edit summary shows +a -d)

### Part 4: Existing tests and verification
- [x] Card-centric tests run in verbose; full `agent_ui`, `acp_thread`, settings suites; clippy; rustfmt check; debug build (done when: all pass)

## Standardized review

### Review: plan: Claude Code style transcript for every agent thread

- **Bottom line:** ready to build; one must-verify on external agents' `kind`, handled by a fallback and M's manual check.
- **Scope:** this plan, the research file, the code locations cited. Not reviewed: ACP adapter sources.

Findings:
1. MUST-VERIFY (goal fidelity): wording depends on `kind` from ACP agents. Owning layer: summary module falls back to the tool title. Need from M: a look at a Claude Code and an OpenCode thread after the build.
2. NOTE (completeness): pixel-level parity with the Claude app is not claimed; theme muted colors are used.

Criterion coverage:
- Goal fidelity: pass. Modes, folding, all agents.
- Current-system accuracy: pass. Claims cite code lines.
- Scope and cohesion: pass. Non-goals explicit.
- Architecture and dependency direction: pass. Wording is a pure module; state in EntryViewState; rendering in thread_view; settings in settings crates.
- Sequencing and dependencies: pass. Setting first, wording second, folding third, tests last.
- Completeness: pass. Settings UI, keymap, search, re-measure, auto-expand covered.
- Failure and recovery: pass. Permission prompts never folded; verbose restores today's look.
- Verification: pass. Each part has a test.
- Feasibility and precision: pass. All named APIs verified.
- Decision readiness: pass. M delegated the open choices; recorded under Decision.

## Changes made

- Part 1: `agent.transcript_view` (normal default) through settings content, resolved settings, default.json and the settings UI dropdown; `agent::CycleTranscriptView` on ctrl-o (macOS, `AcpThread`); "Transcript view" dropdown before the send button, saving to settings; a settings observer re-applies the view to every open thread.
- Part 2: `conversation_view/tool_run_summary.rs` (pure wording) with 7 unit tests.
- Part 3: folding in `render_entry`; summary line (`tool-run-summary`), member lines (`tool-run-member-{ix}`), opened member shows today's rendering; runs re-measure on toggle, all rows on view change; thinking hidden in normal; auto-expand only in verbose; edit line counts from each diff's `BufferDiff`.
- Deviation (structural): the view mode and folding rules live in `EntryViewState`, not `ThreadView`, so thread search uses the same rules as drawing: folded tool labels and hidden thinking are no longer searchable. Three search tests now name the view they test.
- Deviation: debug selectors added to the thinking block and the permission buttons so tests can see them.
- Part 4: 12 card-centric tests set verbose; new tests: fold/open/close, permission never folded (fails without the rule), verbose then switch to normal, normal hides thinking. Suites: `agent_ui` 498, `agent` 761, `agent_settings` 44, `settings` 46, `settings_content` 50, `settings_ui` 57, zed keymap and bundled settings tests 3; clippy clean; rustfmt clean.
- Follow-up: ACP `think` kind is worded by the tool's title like `other` (`verb_for_kind`), because agents send `think` for subagents, task lists and compaction; the Claude Code adapter does (reporters/agent.js, interaction.js, compaction.js), the native agent never does. Claude Code subagents stay a folded line, matching the desktop app (M's call). Test `think_kind_is_told_by_its_title`; `agent_ui` 499; clippy and rustfmt clean.

## Open questions

None.
