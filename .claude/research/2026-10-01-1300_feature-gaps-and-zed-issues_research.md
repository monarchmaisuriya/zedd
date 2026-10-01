# Where zedd can add features, and which Zed bugs to fix

- Date: 2026-10-01 13:00
- Status: presented
- Mode: find out
- Repos: zedd (fork of zed-industries/zed at bd747337d7 + M's commits)
- Question / problem: Which features from Cursor, Claude Code and Fletch are worth adding to zedd, and which serious open Zed bugs can M fix in the fork?

## Research: what to build or fix next in zedd

- **Bottom line:** Four features fit zedd best: (1) the agent's plan as an editable document with approve buttons, (2) diff line comments sent back to the agent, (3) a background-tasks pane, (4) a "done means verified" check that re-prompts once on failure. The adapter or zedd already provides most of the data for these. For bugs, start with Zed #64410: ACP file writes can silently duplicate tokens, the cause is in `acp_thread` and verified, and the fix is small. Confidence: high on what the products offer (primary docs), medium on effort (estimates, not plans).
- **Where we are:** four parallel research passes (Cursor, Claude Code, Fletch, Zed issues) plus code checks in zedd; nothing changed.
- **Need from M:** pick what to take into Plan (recommended: #64410 fix first, then plan-as-document).

### What zedd already has (checked in code, so these are not gaps)

| Capability | Evidence |
| --- | --- |
| Worktree-backed threads ("Create worktree", spawned threads in a fresh worktree) | [verified: crates/agent_ui/src/agent_panel.rs:4881-4911] |
| Agent profiles write / ask / minimal | [verified: assets/settings/default.json:1284-1338] |
| Queued messages while the agent works | [verified: crates/agent_ui/src/conversation_view/message_queue.rs] |
| Notify when the agent waits / is done | [verified: default.json:1350 `notify_when_agent_waiting: primary_screen`] |
| Context usage from ACP `usage_update` | [verified: acp_thread.rs:4002; thread_view.rs `render_token_usage`] |
| Plan entries list from ACP `plan` updates | [verified: thread_view.rs:3911, 4014] |
| Transcript view modes, folded tool calls, thread search, forking, native checkpoints | [verified: zedd PRs #1-#3] |

Not present: hooks for the native agent [verified: no hook code in crates/agent], the adapter's `planFile` capability [verified: acp.rs declares only the fork extension, :5397], `async_task_*` handling [verified: none in acp_thread], side chat [verified: none].

### Feature gaps, ranked by value for effort

| # | Feature | Seen in | What it adds | Needs | Effort | Value |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Plan as an editable document with Approve / Keep planning | Claude Code ([permission-modes](https://code.claude.com/docs/en/permission-modes)), Cursor Plan mode ([modes](https://cursor.com/docs/agent/modes)) | Review and edit the plan before any code is written | Declare the adapter's `planFile` capability, open the file in a buffer, add buttons [verified: adapter docs/air-extensions.md "Plan file"] | S-M | High |
| 2 | Diff line comments sent to the agent | Claude Code desktop ([diff view](https://code.claude.com/docs/en/desktop#review-changes-with-diff-view)) | Review agent edits line by line, send all comments as one prompt | Client only: extend the multi-buffer review | M | High |
| 3 | Background-tasks pane (subagents, background shells) with Stop | Claude Code ([background tasks](https://code.claude.com/docs/en/desktop#watch-background-tasks)) | See and stop what agents run in the background | Client: handle the adapter's `async_task_*` / `subagent_*` events | M | High |
| 4 | "Done means verified" gate | Fletch ([workflow](https://fletch.sh/docs/concepts/workflow/)), Cursor `/goal` | A thread runs a check (tests, new commit) when the agent stops and re-prompts once on failure | Client: verify command per thread | M | High |
| 5 | AI review of the branch before push | Cursor `/review` / Bugbot ([bugbot](https://cursor.com/docs/bugbot)), Claude Code "Review code" | Bugs and security issues before a PR | S as a skill; M with findings inline in the diff | S-M | High |
| 6 | Hooks for the native agent (pre/post tool, stop, prompt submit) | Cursor ([hooks](https://cursor.com/docs/agent/hooks)) | Deterministic guardrails and context injection; Claude Code has its own hooks already | Event points in the native agent loop + JSON-over-stdin runner | M | High (native agent only) |
| 7 | Side chat (ask without derailing) | Claude Code ([side question](https://code.claude.com/docs/en/desktop#ask-a-side-question-without-derailing-the-session)), Cursor `/btw` | Quick question with full context, discarded after | Fork into a popover and drop it | S-M | Medium |
| 8 | Several agent threads visible at once + status sidebar | Cursor Agents Window (changelog 3.0-3.1), Claude Code session sidebar, Fletch | Supervise parallel agents | GPUI panes exist; per-thread status | M | High |
| 9 | Rewind for Claude Code threads | Claude Code ([checkpointing](https://code.claude.com/docs/en/checkpointing)) | Undo code + conversation to a prompt | Adapter exposes no restore method [verified by research agent: adapter src/file-change-audit.ts:135]; client git checkpoints like the native agent | M-L | High |
| 10 | Context usage breakdown by source | Cursor (changelog 3.3, 3.7) | Shows which rules, MCPs, skills fill the context | Client for native; partial for ACP | S-M | Medium |

Lower priority: Debug mode (Cursor), PR/CI status bar via `gh`, session recap, run budgets (Fletch), two-way fork with code-state choice (Fletch), in-editor browser (large; GPUI has no webview). Out of scope for a single developer: cloud agents, Slack/Jira, mobile apps, marketplaces.

### Serious Zed bugs worth fixing (none touched by zedd's own commits)

| # | Issue | Severity | Code location | Root cause | Effort |
| --- | --- | --- | --- | --- | --- |
| 1 | [#64410](https://github.com/zed-industries/zed/issues/64410) ACP `write_text_file` duplicates tokens | Silent source corruption | acp_thread.rs:6118-6212 | Edits are diffed against the agent's last-read snapshot, which is never refreshed after a write [verified: :6135-6151, :6174-6177] | Small |
| 2 | [#62828](https://github.com/zed-industries/zed/issues/62828) Dead ACP connection reused; new threads fail | Agent unusable until restart | agent_connection_store.rs:143-149 | Cached connection never checked for a closed transport [verified by research agent] | Small |
| 3 | [#64538](https://github.com/zed-industries/zed/issues/64538) Unknown terminal id drops a whole session update | Transcript silently incomplete | acp_thread.rs:2866-2915 | `?` on the terminal lookup fails the whole update [verified by research agent]; an upstream test asserts the current behavior | Small |
| 4 | [#60435](https://github.com/zed-industries/zed/issues/60435) Unsent ACP message lost on restart | Typed text lost | draft_prompt_store.rs | Drafts persisted only before a session exists [verified by research agent] | S-M |
| 5 | [#59323](https://github.com/zed-industries/zed/issues/59323) ACP agents SIGKILLed with no grace period | Agents cannot clean up | acp.rs:1509-1515 | `killpg(SIGKILL)` on drop | S-M |
| 6 | [#63202](https://github.com/zed-industries/zed/issues/63202) Sidebar re-lays out every entry each frame (100% CPU while an agent runs) | Battery and heat on macOS | sidebar.rs:2215 | Spinner animation forces full list re-render | M |
| 7 | [#63177](https://github.com/zed-industries/zed/issues/63177) Save truncates before writing; crash leaves an empty file | Data loss | fs.rs:1019-1035 | In-place create+write; `atomic_write` exists alongside | M (hardlink/permission care) |
| 8 | [#55726](https://github.com/zed-industries/zed/issues/55726) Unsaved buffers lost when launched by opening a file | Data loss | main.rs:927-951 | Workspace restore skipped on open requests | M |

Skipped because the cause is in the Claude adapter, not Zed: #63867 (Claude threads wedge), #64686 (Claude replies duplicated with `<cc-memory>`; adapter `unstreamedRemainder`). Skipped because a PR is already open: #64661, #63743, #64611, #62631.

### How it fits together

```
next work in zedd
  bugs (small, verified)  -> #64410 snapshot refresh -> #62828 stale connection -> #64538
  features (data exists)  -> plan document (planFile) -> diff line comments -> tasks pane
  features (new client)   -> verify gate -> branch review skill -> hooks (native)
```

### Confidence and gaps

| Claim | Tag | Source tier | What would raise confidence |
| --- | --- | --- | --- |
| Product features (Cursor, Claude Code, Fletch) | verified | Primary docs and changelogs (fetched through a summarizer) | Spot-check exact wording on the pages |
| zedd has / lacks the listed items | verified for the rows marked so | zedd code | n/a |
| #64410 mechanism | verified (code); repro inferred | Code + issue | Two-write regression test |
| #62828, #64538, #60435 root causes | verified by the research agent, not re-read by me | Code + issue | Read before planning |
| Effort and value | inferred | Estimates | A plan per item |
| Whether the Claude adapter uses `write_text_file` | unverified | n/a | ACP log of a Claude edit |

### Sources

- Cursor: https://cursor.com/features, https://cursor.com/changelog (pages 1-14), https://cursor.com/docs/agent/modes, https://cursor.com/docs/agent/hooks, https://cursor.com/docs/configuration/worktrees, https://cursor.com/docs/bugbot, https://cursor.com/docs/agent/debug-mode
- Claude Code: https://claude.com/product/claude-code, https://code.claude.com/docs/en/desktop, https://code.claude.com/docs/en/permission-modes, https://code.claude.com/docs/en/checkpointing, https://code.claude.com/docs/en/worktrees, https://code.claude.com/docs/en/interactive-mode, https://github.com/agentclientprotocol/claude-agent-acp (docs/air-extensions.md)
- Fletch: https://fletch.sh/, https://fletch.sh/docs/concepts/workflow/, https://fletch.sh/docs/concepts/parallel-agents/, https://fletch.sh/docs/guides/forking/, https://fletch.sh/docs/guides/pull-requests-and-ci/, https://fletch.sh/changelog, https://github.com/fwdai/fletch
- Zed issues: https://github.com/zed-industries/zed/issues/64410, /62828, /64538, /60435, /59323, /63202, /63177, /55726

## Standardized review

### Review: research: where zedd can add features, and which Zed bugs to fix

- **Bottom line:** decision-grade for choosing what to plan next; effort figures are estimates and must be confirmed by a plan.
- **Scope:** this file, the four research reports, and code checks in zedd (agent_panel.rs, default.json, message_queue.rs, acp_thread.rs, acp.rs). Not re-read: the code locations for bugs 2-8.

Findings:
1. MUST-VERIFY: bugs 2-8 root causes rest on the research agent's code reading; only #64410 was re-read. Owning layer: read each before its plan.
2. NOTE: product pages were read through a summarizing fetcher, so quotes are close but may not be word for word.
3. NOTE: the Cursor and Claude Code passes marked several zedd statuses as unverified; this file replaces them with code-checked ones where it says "verified".

Criterion coverage:
- Question and decision fit: pass. Method: pass (four parallel passes, code checks). Source quality: pass (primary docs, changelogs, issue tracker, source). Coverage: pass (three products, eight bugs, skipped list with reasons). Claim traceability: pass. Evidence handling: pass (verified vs inferred marked). Analysis quality: pass (same value/effort scale). Codebase fit: pass (existing zedd capabilities removed from the gap list). Uncertainty: pass (confidence table). Recommendation proportionality: pass. Reproducibility: pass (URLs and file lines).
