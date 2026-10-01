# Show a lone tool call directly in the folded transcript

- Date: 2026-10-01 11:00
- Status: done (debug check pending M)
- Repos: zedd   Branch: zedd-claude-code-transcript (PR #3)
- Folders: crates/agent_ui, crates/acp_thread
- Files: conversation_view/thread_view.rs, entry_view_state.rs, conversation_view/tool_run_summary.rs (tests only if needed), acp_thread/src/diff.rs, conversation_view.rs (tests)
- Subagents: none

## Context and problem

M's screenshot: a run holding one edit draws "Edited a file +228 -97 ⌄", then the same line again for the tool, then the diff card. With one tool the run line and the tool line say the same thing. The line also says "a file" while the card knows the path.

## Goals / Non-goals

Goals:
- A run of exactly one tool call draws one line, worded for that tool ("Ran cargo test", "Edited plan.md +228 -97"); clicking it opens that tool's full rendering directly.
- File tools name their file from the diff when the agent sends no location.

Non-goals: changing runs of two or more tools.

## What is true today

- The first entry of a run draws the run line; an open run adds a member line per tool, and an open member adds the full rendering [verified: thread_view.rs `render_tool_run_entry`, `render_tool_run_member`].
- File names come only from `tool_call.locations` [verified: thread_view.rs `tool_summary_item`]; the native edit tool reports its path through its `Diff` (`Diff::file_path`) [verified: acp_thread/src/diff.rs:411].
- ACP v2 diff paths are behind a private helper `diff_change_paths` [verified: diff.rs:239].
- Invariant kept by `EntryViewState`: a tool counts as shown only when its run is open; closing a run closes its tools [verified: entry_view_state.rs `toggle_tool_run`].

## Decision

- One-tool run: the run line uses the member wording, and clicking it opens or closes the run and the tool together (`toggle_single_tool_run`), so the invariant holds and a run that later grows to two tools is already open with its first tool shown.
- File name fallback: locations, then the first diff's path (`Diff::file_path`, or the v2 change path through a new public `diff_change_path` next to `diff_change_label`).

## Design

```
run with 1 tool:  "Ran cargo test ›"  --click-->  full rendering below      <-- changed
run with 2+ tools: unchanged ("Read 2 files ›" -> member lines -> rendering)
```

## Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| A one-tool run grows while open | Opening sets both run and tool open, so the grown run shows its member lines |
| Search sees hidden content | Same state as before: tool open implies run open |

## Acceptance criteria

- Given a run with one tool, when it renders, then one line shows and no member line; when clicked, the tool's output shows; when clicked again, it hides.
- Given an edit with a diff and no location, then the line names the file.

## Parts

### Part 1: One-tool runs
- [x] `EntryViewState::toggle_single_tool_run`; single-tool rendering in `render_tool_run_entry` (done when: GPUI test for one-tool run passes)

### Part 2: File name from diffs
- [x] `acp_thread::diff_change_path`; diff fallback in `tool_summary_item` (done when: test with a diff and no location names the file)

### Part 3: Verify
- [x] `agent_ui`, `acp_thread` suites, clippy, rustfmt check, debug build

## Standardized review

- **Bottom line:** small, cohesive, ready; no blocking findings.
- Goal fidelity: pass (one line per lone tool, opens directly). Current-system accuracy: pass (cited). Scope: pass (multi-tool runs untouched). Architecture: pass (state in EntryViewState, path accessor in acp_thread next to its sibling). Failure and recovery: pass (growth case handled by opening both). Verification: pass (a test per part). Decision readiness: pass.

## Changes made

- Part 1: `EntryViewState::toggle_single_tool_run` opens the run and its tool together; a one-tool run's line uses the member wording and opens straight to the tool's rendering. Test `test_lone_tool_call_opens_directly` (fails on the old behavior, which showed a member line).
- Part 2: `acp_thread::diff_change_path`; file tools fall back to their diff's path (`Diff::file_path`, v2 change path). Test `test_edit_without_location_names_its_file_from_its_diff` (fails with the fallback off).
- Part 3: `agent_ui` 501, `acp_thread` 218; clippy clean; rustfmt clean; debug build passes.

## Open questions

None.
