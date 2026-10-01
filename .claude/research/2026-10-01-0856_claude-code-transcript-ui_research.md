# Claude Code desktop transcript UI for the zedd agent panel

- Date: 2026-10-01 08:56
- Status: presented
- Mode: explore options
- Repos: zedd   Folders: crates/agent_ui, crates/acp_thread, crates/agent_settings, crates/settings_content, assets
- Question / problem: How does the Claude Code desktop app lay out its chat transcript, and how do we give every zedd agent thread (native and external ACP) the same layout?

## Options: make every zedd thread read like the Claude Code desktop transcript, with tool calls folded into one-line summaries, by changing how the existing thread entries are drawn

- **Bottom line:** Add Claude Code's three transcript view modes (Normal, Thinking, Verbose) and draw each run of consecutive tool calls as one muted summary line ("Read 9 files ›", "Edited globals.css +3 -0 ›") that expands in place. Build it at the render layer over the existing thread entries, so it works for the native agent and every ACP agent with no protocol change.
- **Where we are:** framing done; 7 sources plus 8 video frames read; zedd's renderer mapped; next step is the plan.
- **Need from M:** pick an option, and answer the scope questions at the end.

### What the Claude Code desktop transcript looks like

Evidence from the official docs and from frames of an Academind walkthrough of the app (video chapter "Interface and agent workflow", frames at 3:28, 3:45, 4:05, 4:22, 4:44):

- **Three view modes** [verified: [docs, Switch view modes](https://code.claude.com/docs/en/desktop#switch-view-modes)]: Normal ("Tool calls collapsed into summaries, with full text responses"), Thinking (same plus thinking), Verbose (every tool call and step plus thinking). A "Transcript view" dropdown sits next to the send button; Ctrl+O cycles modes.
- **One muted line per run of tool calls** [verified: video 3:28]. Between two pieces of assistant text, all tool calls collapse into one small gray line with a `›` chevron. Seen wording:
  - "Searched code, found files, read a file ›" (mixed kinds, joined with commas, lowercase after the first)
  - "Searched code, ran an agent ›"
  - "Read 9 files ›"
  - "Edited globals.css +3 -0 ›" (one file: name in code font, green/red line counts)
  - "Edited 9 files ›", "Edited 3 files ›", "Edited 2 files ›"
  - "Searched blue-[567]00 ›" (one search: the query itself)
  - "Used ToolSearch ›", "Used preview start ›" (tools without a natural verb)
  - "Asked Dev servers ⌄" (a question to the user)
- **The live line is present tense** [verified: video 4:05]: "Reading package.json ›" while the tool runs.
- **Expanding a line shows the tool's input as plain muted text under it**, no card [verified: video 4:22, "Asked Dev servers ⌄" followed by the raw question JSON].
- **No cards, borders or icons on tool lines** [verified: video 3:28, 3:45]. Assistant text is normal prose with bold headings and inline code; the user prompt is a tinted rounded box [verified: video 3:28 top].
- **Working indicator** at the end of the transcript: an orange spinner glyph, elapsed time and streamed tokens, "8s · ↓151 tokens", "48s · ↓891 tokens" [verified: video 4:05, 4:22].
- **Grouping is per run between texts**, and the fold label is generated, e.g. "Used N tools" [verified: [issue #94354](https://github.com/anthropics/claude-code/issues/94354), secondary]. That issue also reports the desktop app hides mid-turn assistant text in favor of summaries; we will not copy that (core rule: full text responses stay visible, which matches the docs' own Normal definition).
- **Subagents show as one line**, e.g. "Ran agent Haiku 4.5 Explore MCP server codebase" [verified: [kiloloco write-up](https://www.kiloloco.com/articles/claude-code-desktop-first-impressions), secondary].
- **Diff review lives outside the transcript**: a `+12 -1` indicator opens a diff pane [verified: [docs, diff view](https://code.claude.com/docs/en/desktop#review-changes-with-diff-view)].
- Requested but not shipped: collapsing a whole finished turn into "Worked for 4m 12s · 23 tool calls" [verified: [issue #96497](https://github.com/anthropics/claude-code/issues/96497)]. Out of scope.

Gaps: I could not screenshot the Claude app directly (it is not reachable by the screenshot tool from inside its own session). Exact colors, spacing and the Execute wording ("Ran a command" vs the command text) were not visible in the frames and are marked unverified below.

### The problem, framed (6W2H)

- **What:** zedd draws every tool call as its own card or icon row, so a working thread is a wall of boxes (M's screenshot: three gray command boxes in a row).
- **Why:** M wants to read what the agent is doing at a glance, like Claude Code desktop, without losing the ability to drill in.
- **Who:** M, using the native Zed agent, Claude Code over ACP, and OpenCode.
- **Whom:** the reader of the thread.
- **When:** every turn; most visible during long tool-heavy turns.
- **Where:** `crates/agent_ui/src/conversation_view/thread_view.rs` (render), `entry_view_state.rs` (fold state), agent settings, the message editor toolbar.
- **Which:** GPUI rendering over `AcpThread` entries; no protocol change.
- **How:** group consecutive tool-call entries at render time; first entry draws the summary line, the rest draw nothing while folded.
- **How much:** roughly 1,000 to 1,500 changed lines, mostly in `thread_view.rs`, plus one new summary module and its tests. Medium-high blast radius (the most-used UI in the panel); reversible by setting the mode to Verbose.

Refocused problem: give every thread a Normal view where each run of tool calls is one muted, expandable summary line, with Verbose keeping today's full cards.

### What must be true

Hard constraints:
- Works for native and external agents from fields both provide: `kind`, `label`, `status`, `locations`, diffs, terminals [verified: `acp_thread.rs:1019-1044`, code map].
- A tool call waiting for permission is never folded away; its buttons stay visible [verified: permission row is a separate floating layout today, `thread_view.rs:3755`; folding must not hide the inline one either].
- A failed tool call stays noticeable when folded.
- Assistant text and user prompts are never folded.
- The list stays one row per entry: `render_entries` builds one list item per entry index [verified: `thread_view.rs:6218-6248`]. Hidden members of a fold must draw an empty element, which already happens for canceled empty tool calls [verified: `thread_view.rs:6554-6576`].

Conventions:
- Settings follow the content/resolved/default.json/settings UI pattern (as `tool_output_preview_lines` did).
- Fold state lives in `EntryViewState` next to `expanded_tool_calls` [verified: `entry_view_state.rs:40-58`].

Open checks:
- [unverified] The Claude Code ACP adapter and OpenCode set `kind` on each tool call (the adapters are not in this repo). Check: log `tool_call.kind()` for a few calls in a real thread of each agent.
- [unverified] Execute wording in Claude Code ("Ran a command" or the command text). Check: a Claude Code desktop screenshot from M.
- [unverified] Whether zedd has streamed-token counts for the working indicator. Check: grep the thread for token usage during generation.

### Options

| | A: View modes + folded summary lines at render time (recommended) | B: One-line rows per tool, no folding | C: Separate display-row model (groups become their own list items) |
| --- | --- | --- | --- |
| How it works | New `agent.transcript_view` (normal / thinking / verbose). In Normal and Thinking, each run of consecutive tool calls draws one summary line at its first entry; other members draw nothing until expanded. Expanded: one sub-line per tool, each expands to its input/output. Verbose: today's cards. | Restyle each tool call as its own muted one-line row with a chevron; no grouping, no modes. | A new view model maps entries to display rows; the GPUI list is sized by rows; groups are first-class rows. |
| Codebase fit | Keeps one row per entry and reuses existing content renderers, the empty-element pattern, and `EntryViewState`. | Smallest change to `render_tool_call` / `render_tool_call_label`. | Fights the codebase: `entry_ix` is used by search, fork points, checkpoints, scroll restore and list splicing. |
| Cost and blast radius | ~1,000-1,500 lines; most-used panel UI; reversible via Verbose. | ~400 lines; low. | ~2,500+ lines; high; touches list splice and scroll anchoring. |
| Fails when | Grouping rule wrong (e.g. hidden thinking-only entries splitting a run): must treat hidden entries as transparent. | A turn with 20 tool calls is still 20 lines; does not match "Read 9 files". | Index translation bugs between rows and entries. |
| Evidence | Docs view modes; video frames; existing empty-entry pattern. | Video shows grouping, so B misses the core behavior. | Code map: entry index is the shared key across features. |
| Completeness | 9/10 | 5/10 | 9/10 |

### Flow of the recommended option

```
AcpThread entries (unchanged)
  |
  v
render_entry(ix)
  |-- user message ............ tinted bubble                      <-- changed
  |-- assistant text .......... prose (thought chunks: by mode)     <-- changed
  |-- tool call
        |-- verbose ........... today's card (unchanged)
        |-- normal / thinking
              |-- first of run  -> summary line "Read 9 files ›"   <-- new
              |-- rest of run   -> empty (or sub-line when open)    <-- new
              |-- waiting for permission -> always full card
```

### Recommendation

RECOMMENDATION: A because it matches Claude Code's documented modes and the folded lines seen in the app, works for every agent from data they already send, and keeps the one-row-per-entry list that search, fork and scroll depend on.

### Scope questions for M (before the plan)

1. Default mode: Normal (like Claude Code) or Verbose (today's zedd look)? Recommendation: Normal.
2. Include the working indicator ("✳ 48s · ↓891 tokens") and the user-prompt bubble restyle in this change, or transcript tool lines and modes only? Recommendation: include both; they are small.
3. Composer (prompt box, mode and model pickers) is out of scope unless M wants it.
4. Thread search: folded tool content is not searched (matches today's rule that only open content is searched), assistant text always is. OK?

### Sources

- Claude Code Docs, Desktop application (primary): https://code.claude.com/docs/en/desktop
- Claude Code Docs, Desktop quickstart (primary): https://code.claude.com/docs/en/desktop-quickstart
- Academind, "Key features of the NEW Claude Code desktop app you MUST know!" (primary visual, frames 3:16-4:44): https://www.youtube.com/watch?v=N9nU9oLZ30o
- anthropics/claude-code issue #94354, summary lines replace mid-turn text: https://github.com/anthropics/claude-code/issues/94354
- anthropics/claude-code issue #96497, collapse finished turns request: https://github.com/anthropics/claude-code/issues/96497
- anthropics/claude-code issue #76577, persist transcript view mode: https://github.com/anthropics/claude-code/issues/76577
- Kiloloco, Claude Code Desktop first impressions: https://www.kiloloco.com/articles/claude-code-desktop-first-impressions
- ZayRTun/zua issue #1, Claude Code style transcript (adjacent, CLI style): https://github.com/ZayRTun/zua/issues/1

## Standardized review

### Review: research: Claude Code desktop transcript UI for the zedd agent panel

- **Bottom line:** usable for a decision; one must-verify (agent `kind` coverage) and one note (visual details not fully observed).
- **Scope:** this file, the cited docs and issues, 8 video frames, and the zedd renderer code map. Not reviewed: the Claude Code ACP adapter source, Claude app internals.

Findings:

1. MUST-VERIFY: the folded wording depends on `kind` being set by external agents. If an adapter sends everything as `Other`, every line reads "Used <tool>". Owning layer: the summary builder must fall back to the tool's label, and the plan must include a check against a real Claude Code and OpenCode thread. Need from M: nothing now; it is a plan task.
2. NOTE: exact colors, spacing and Execute wording were not observable. The plan should use the theme's muted text color and say so, rather than claim pixel parity.

Criterion coverage:
- Question and decision fit: pass. Question, decision (option pick) and scope questions are explicit.
- Method: pass. Docs first, then video frames, then issues; frames and timestamps recorded.
- Source quality: pass. Official docs anchor the modes; the video is direct visual evidence; issues are secondary and labeled.
- Coverage and alternatives: pass. Three options including a minimal one and a heavy one.
- Claim traceability: pass. Each UI claim cites docs, a frame time, or an issue.
- Evidence handling: pass. Unverified items listed with checks.
- Analysis quality: pass. Same criteria for all options.
- Codebase and system fit: pass. Cites the one-row-per-entry list and the empty-entry precedent.
- Uncertainty and limits: pass. Gaps section names what could not be seen.
- Recommendation proportionality: pass. Recommends A and names what would change it (adapter `kind` coverage).
- Reproducibility: pass. Video URL and timestamps, doc anchors and code lines given.
