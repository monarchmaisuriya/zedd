# Minimal formatted view for subagent reports and task notifications

- Date: 2026-10-01 11:30
- Status: done in zedd (debug check pending M); upstream PR amended to tag instead of skip
- Repos: zedd   Branch: zedd-claude-code-transcript (PR #3)
- Folders: crates/acp_thread, crates/agent_servers, crates/agent_ui
- Files: acp_thread/src/acp_thread.rs, agent_servers/src/acp.rs, agent_ui/src/entry_view_state.rs, agent_ui/src/conversation_view/thread_view.rs, agent_ui/src/conversation_view/thread_search_bar.rs, agent_ui/src/conversation_view.rs (tests)
- Subagents: none

## Context and problem

The stopgap in `acp.rs` drops the subagent's hand-back and task notifications from replayed Claude Code threads, so the subagent's report is gone entirely (M: "you completely removed sub agent view"). Before that they rendered as giant raw user-message boxes. M wants a minimal, formatted view.

## Goals / Non-goals

Goals:
- A task notification is one muted line: `Agent "Web research bot detection" finished · 78 tools · 5m 48s`.
- A subagent report is one muted line, `Report from subagent: <first heading>  ›`, that expands to the report rendered as markdown (preamble removed, indentation undone).
- Neither renders as a user message (no editor box, edit, checkpoint or fork controls).
- Search matches only what is drawn: the report text only while expanded.

Non-goals: live sessions (the adapter never sends these turns live); other agents.

## What is true today

- Stopgap drops the two framings for the Claude adapter [verified: acp.rs `is_injected_claude_user_turn`].
- `UserMessage.meta` exists and is filled by the keyed (v2) path, but the legacy `UserMessageChunk` path builds the entry with `meta: None` [verified: acp_thread.rs:301, 4342, 3871-3900].
- User messages render through a `MessageEditor` and search reads that editor [verified: thread_view.rs render_entry UserMessage branch; thread_search_bar.rs:348-361].
- Compaction notices keep expansion by entry index with re-indexing on removal [verified: entry_view_state.rs:312-324, 712-716].

## Decision

- The ACP layer (Claude adapter only) rewrites a recognized injected turn into its clean form and tags it with `_meta["zed_injected_turn"]`: `{ "kind": "subagent_report", "title": … }` with the report markdown as text, or `{ "kind": "task_notification" }` with the one-line summary as text.
- `acp_thread` keeps a legacy user chunk's `_meta` on the message it starts, and defines the shared key and the `InjectedTurn` type (beside `SUBAGENT_SESSION_INFO_META_KEY`).
- The thread view renders a tagged user message as a notice line; expansion state lives in `EntryViewState` like compactions.

## Design

```
adapter replay  "Another Claude session sent a message: <agent-message …> report"
      v
acp.rs (Claude adapter)  rewrite + tag _meta.zed_injected_turn        <-- changed
      v
acp_thread  keep chunk _meta on the new UserMessage                   <-- changed
      v
thread_view  tagged -> "Report from subagent: X ›" -> markdown        <-- new
             task   -> "Agent "X" finished · 78 tools · 5m 48s"       <-- new
```

## Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Framing changes in a future Claude Code | Unrecognized text falls back to today's user message; labeled stopgap with the upstream issue |
| Upstream PR #1204 (skip on replay) would remove the data again | Amend #1204 to forward these turns tagged with their origin instead of skipping (needs M's OK; see Open questions) |
| Injected user message still counts as a turn boundary | Accepted: it was a boundary before too |

## Acceptance criteria

- Given a replayed hand-back, then one "Report from subagent: …" line shows; when clicked, the report renders as markdown; when clicked again, it hides.
- Given a replayed task notification, then one "Agent … finished · …" line shows.
- Given a real prompt, then it still renders as a user message.

## Parts

### Part 1: Data
- [x] acp_thread: keep legacy user chunk `_meta`; `INJECTED_TURN_META_KEY`, `InjectedTurn`, `UserMessage::injected_turn` (done when: acp_thread test shows meta kept)
- [x] acp.rs: parse the two framings into clean text + tag, replacing the drop (done when: unit tests for report and notification parsing)

### Part 2: View
- [x] EntryViewState expansion set with re-indexing; notice rendering; search rule (done when: GPUI tests for report expand/collapse and notification line)

### Part 3: Verify
- [x] acp_thread, agent_servers, agent_ui suites; clippy; rustfmt check; debug build

## Standardized review

- **Bottom line:** ready; one decision for M on the upstream PR.
- Goal fidelity: pass. Current-system accuracy: pass (cited). Architecture: pass with note: parsing harness framing stays in the labeled ACP stopgap; the model and view only know a generic "injected turn" tag. Dependency direction: pass (key and type in acp_thread, used by agent_servers and agent_ui). Failure: pass (unrecognized text falls back). Verification: pass. Decision readiness: must-verify, upstream PR direction (Open questions).

## Changes made

- Part 1: `acp_thread` keeps a legacy user chunk's `_meta` on the message it starts; `INJECTED_TURN_META_KEY`, `InjectedTurn`, `UserMessage::injected_turn`. `acp.rs` rewrites a recognized hand-back into its report markdown (preamble and harness indent removed, title from its first heading) and a task notification into one line (`summary · N tools · duration`), tagged, instead of dropping them. Tests: hand-back becomes a report notice, notification becomes one line, real prompts and agent text untouched. Checked on the real hand-back: title "Research report: how the industry detects bots, applied to a client-side JS SDK on third-party storefronts (as of Sept 2026)".
- Part 2: `EntryViewState::expanded_subagent_reports` (re-indexed on removal); `render_injected_turn` draws the notification line and the report line that opens to rendered markdown; search skips injected turns except an opened report's text. Test `test_injected_turns_render_as_notices` (fails if `_meta` is dropped or the turn renders as a user message).
- Part 3: `acp_thread` 218, `agent_servers` 46, `agent_ui` 502; clippy clean; rustfmt clean; debug build passes.

## Open questions

None. Upstream PR #1204 was amended (M's OK) to tag these turns with `_meta["_claude/origin"]` on replay instead of skipping them, so this view keeps its data after the adapter ships the fix; the zedd stopgap can then key on that tag instead of the framing.
