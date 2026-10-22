<!--
  Licensed to the Apache Software Foundation (ASF) under one
  or more contributor license agreements.  See the NOTICE file
  distributed with this work for additional information
  regarding copyright ownership.  The ASF licenses this file
  to you under the Apache License, Version 2.0 (the
  "License"); you may not use this file except in compliance
  with the License.  You may obtain a copy of the License at

      http://www.apache.org/licenses/LICENSE-2.0

  Unless required by applicable law or agreed to in writing,
  software distributed under the License is distributed on an
  "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
  KIND, either express or implied.  See the License for the
  specific language governing permissions and limitations
  under the License.
-->

# Phase 4: search, the terminal and the workbar, learning from Zed

Status: started 2026-10-10. The user's goal: "参考 zed 的一些做法，把终端、搜索等模块都做了" — build
the terminal, search and similar modules, learning from Zed's designs. Decisions are made between
two reviewers (Opus and Fable) without a sign-off per step; each package is committed on its own.

## Decisions

- **Learn from Zed, copy nothing.** Zed's `terminal`, `terminal_view`, `search` and `workspace`
  crates are GPL-3.0-or-later; only their practices are used. `gpui` and `alacritty_terminal`
  are Apache-2.0.
- **Find in the conversation (⌘F)** matches the transcript model, not the view: the transcript's
  text state exists only for rows the list has laid out. The visible text of each message (no
  markup, no link targets) is matched off the main thread, mapped to the rendered text with
  gpui-kit's `range_for_source` when a row paints, and painted with `set_range_highlights`.
  Collapsed tool output and reasoning are searched and open when a match in them becomes active.
  Case-sensitive and whole-word options, no regex. Opening the bar reads the rest of the history
  in the background (closing it aborts the page in flight). The `search` crate holds a minimal
  Zed-shaped `Searchable` trait that the terminal implements too.
- **Search every task (⇧⌘F)** is a page in the plate over `recall.query`, not a modal (a modal
  loses the results when a task opens) and not the ⌘K palette (which stays commands and titles).
  Opening a passage opens its task at the anchor message with the find bar on its term; Back
  returns to the results.
- **The terminal's PTY belongs to the Runtime Host** (`runtime.resource.*`), as in Desktop: it
  outlives the window, the task switch and the panel. The client emulates the terminal with
  `alacritty_terminal` fed by the Host's UTF-8 output, painted the Zed way (merged runs at a
  forced cell width, merged background rects, box drawing as quads). The client keeps under the
  Host's live-PTY cap of 8 itself: over it the Host answers a generic failure and drains (see
  Upstream).
- **The workbar** is the right panel turned into tool tabs (Changes, Files, Trace, Side chat,
  Terminal), one panel with the existing placements, not a bottom dock: the conversation column
  is narrow and its height is the scarce axis.
- **Files (⌘P)** shows what Desktop's Files tool shows (subagent writebacks, Deep Research
  reports, a tool's HTML), polled with a `get` every 2 s while the face shows, the list read
  again only when the revision moves. Large text is read in chunks (Show more, Show all, up to
  2 MB), images in chunks with their format told from their bytes. An HTML page renders in the
  face through gpui-kit's HTML text view, as text with no scripts and no CSS, up to 256 KB
  (Desktop's bound) and read whole; past that it shows as source. No webview (see Later
  candidates). Links in rendered text open only `http:`, `https:` and `mailto:` (Desktop's
  rule); a page's images draw only from `data:` URLs, as the app has no HTTP client. Open in
  Default App for PDFs, raster images and HTML pages (the browser runs a page's scripts), as a
  copy under the app's cache named by ids; Save As for every kind; no Show in Finder for an
  Artifact (no path in the protocol).
- **Trace (Desktop's Inspector)** is a workbar face after Files: persisted for the task, one
  face, the `activity` glyph (added to the icon set), no shortcut, as Desktop's. It shows what
  Desktop's panel shows from the same three reads, `execution.inspect.query`
  (`session_trace_start`, then `session_trace_continue` from the last page's cursor),
  `usage.query` `summary` for the Session, and `context.diagnostics.query`: the token split a
  bill reads, the time split, the estimated cost (unknown, never `$0.00`, when nothing is
  priced), the context window from the latest settled request's own window and its byte
  composition, then the Turns newest first. Desktop's models are ported with its test cases.
  Two authorities refresh it, kept apart: the Session's frames (Tool events, durable rows,
  projections; never deltas) re-read the trace and the snapshot, `session_domain_changed` with
  domain `usage` the summary alone; a burst once after 400 ms, nothing while the face is
  hidden. Load earlier keeps its depth for the task across refreshes and re-activation.
  Differences from Desktop: the token and time splits are bars like the context window's, not
  rings; a Turn folds to one line (the newest opens) where Desktop lists every step; the
  composer's context gauge (`live-context-usage`, `latest-request-usage`) is not here, as the
  composer has none.
- **Side chat** is a workbar tool after Trace (Desktop's `side-chat`): several faces, numbered
  "Side chat", "Side chat 2" as Desktop numbers them, not kept across runs, the set's new `chat`
  glyph (Desktop's `message-circle-question`), ⌥⌘S. It is a side conversation about the task
  that reads the task's history and does not change the task: on its first send it forks the
  task with `session.branch.create` and the side-conversation intent, through the task's latest
  completed Turn from `session.turns.query` (empty when none completed), sending the create again
  at the revision the Host names on a conflict, mapping `session_busy` and
  `operation_unavailable` to Desktop's words, and `operation_conflict` (raised before the Host
  makes anything) to setup failed, with a new target on the next send. The fork's copied Turns
  are its model's context and show no rows. A side chat belongs to its task and lives while the
  app runs; only the
  selected task's side chats have tabs, and coming back shows them as they were. Closing one
  with a conversation asks first (Desktop's confirmation and its Don't ask again, kept in the
  preferences); deleting its task, not archiving it, disposes of it. Disposal stops the fork's
  runs, then `session.remove`. Because the app can stop between a create and a removal, every
  fork is written in an app-wide ledger (`side-chat-forks.json` beside the preferences) with the
  State Root's id, the whole creation input and its phase, flushed before the create goes, as
  Desktop's cleanup authority does; each run owns its entries through a lock file of its own.
  Every save reads the file again under a lock and keeps other runs' entries as they are on
  disk, so two runs on one config directory keep each other's; a document that does not parse
  is moved aside, and one that cannot be read at launch is not written that run.
  When a window connects to a root, the entries for it that no live side chat holds and whose
  run has ended are settled: one still `creating` has its create sent again first (it may have
  committed after the run stopped), then the fork is removed; `not_found` settles it. A removal
  that gives up is tried again a minute later while the root stays connected. Quitting does not
  dispose of side chats: their forks settle at the next launch once that root connects, as
  Desktop's `recover()` does at boot. Forks the ledger does not hold are never touched. The
  panel is the fork's transcript and a composer of
  its own at the panel's width (send, stop, steering, queued follow-ups, attachments, prompts as
  in the task's conversation; the permission mode changeable, before the fork the mode it starts
  with; the model read-only; no project chip). ⌥⌘S with text selected in the task's
  conversation, and the transcript's context menu item Ask in side chat, stage it as a quote
  chip above the side composer (`sourceTurnId` when one Turn's replies hold the whole selection;
  the kit tells which reply rows hold it); quotes go with the next message and stay when it
  fails; user rows show the quotes they carry. Forks are hidden from the sidebar and ⌘K (their
  label as well as their parent), the Search page, and run notifications; their spend stays on
  the Usage page, as Desktop's. Differences from Desktop: the tool comes after Trace rather than
  first; a selection's quote carries no label (Desktop derives none either), and a selection
  across Turns still quotes, without its Turn, where Desktop offers no quote; the message shows
  once the Host's transcript has it, with no optimistic bubble, as in the task's conversation;
  the composer is not disabled when the catalog no longer offers the model (the Host's refusal
  shows, as in the main composer), though an empty fork whose model went away is replaced from
  the task as Desktop does; no `/compact` (the main composer has none) and no quote notes; a
  fork's Usage row opens the task it forks, as this client never shows a fork as a task; no
  WorkHub coordination source, as this client shows none.
- **Syntax colours** on every code surface (the transcript's tagged code blocks, the changes
  panel's and tool cards' diffs by extension, the Files source view by extension or media type),
  from gpui-kit's tree-sitter highlighter with a curated set of 18 grammars, not the kit's whole
  list. The colours are each palette's own syntax roles (`shared::theme::SyntaxPalette`): one hue
  per role across palettes, at a lightness per mode; variables in the ink; comments, punctuation
  and operators mixed from the palette's ink and background. Every role keeps APCA Lc 45 on the
  fills code sits on (code, sunken, a diff's added and removed rows) in every palette and mode,
  comments Lc 35.
- **Paths in replies (F46)** open on a click, on a Host on this machine only (Zed's agent panel
  does this; Desktop does not). When a reply settles, one background pass checks each inline
  code span that is wholly a path (absolute, `~/…`, or relative to the task's folder; a trailing
  `:line[:col]` dropped) and each link to a `file:` URL or a plain path. Those naming something
  that exists become links with the client's own `maka-file:` scheme in the Markdown handed to
  the text view, so they draw as the kit's link over a code chip; find searches the reply's own
  text and maps its ranges onto the rewritten one. A click decides again from the file there,
  symlinks resolved: nothing there says so; a folder, a bundle or an unknown kind shows in
  Finder; a document on a fixed list (Office, iWork, OpenDocument, PDF, CSV, RTF, EPUB) opens in
  the default app; text, source, an HTML page or an image within the Files face's limits
  previews there, with Show in Finder and Copy Path. Nothing else is opened or launched. No
  right-click menu or tooltip on a path: the kit's text view has no hook for either.
- **The prompt rail (F47)** is Desktop's (`prompt-anchor-rail.tsx`): a tick per Turn whose
  prompt has text (Desktop leaves out a prompt of files only, as the Host's index does), sampled
  to 64 past that, the tick of the Turn at the top of the transcript widest; a card after 120 ms
  with the prompt and the first 140 characters of the final reply; a click scrolls the prompt to
  the top. Turns the transcript does not hold come from `session.turn_landmarks.query`, read
  once per transcript while older history lies outside it, and a click on one reads older pages
  until it is held, as `open_at_passage` does. The kit's `MessageScroller` reports no scroll
  position, so the Turn being read comes from the bounds the rows painted at (`Paging` records
  them), read after the list paints and taken a frame later. The rail rests 16 px in, clear of
  the list's scrollbar track (Desktop: 12 px, clear of macOS's overlay scrollbar). Desktop shows
  its rail from an 825 px window, workbar open or not; here the task's conversation keeps the
  hovered rail's 53 px right of its reading column from a 560 px transcript (the column keeps
  483 px), whatever the number of prompts, so nothing moves when the third prompt brings the
  rail: the column stays centred while that leaves 53 px beside it (from 826 px); narrower, it
  keeps 53 px on the right and at least 24 on the left, and narrows once both are reached (from
  797 px down, 24 and 53 exactly). The transcript's rows, its status line, the strip over the
  composer and the composer (in a draft too, so the first send moves nothing) share that column;
  the gutters are measured from the last frame's width. Under 560 px the gutters are even and
  the rail hides. The kit's HoverCard opens for the pointer only, so a tick reached with Tab
  says its prompt through its name and shows no card; the kit's Button carries no accessible
  current or selected state, so which tick is active shows only on screen. A side chat has no
  rail.
- **A closed terminal's tab goes at once (F48)**, as in Zed and Alacritty, not when the Host
  answers the stop as in Desktop. The Host's `runtime.resource.stop` sends SIGTERM to the shell's
  process group, waits up to 2 s (`DEFAULT_PROCESS_TERMINATION_GRACE_MS`) for it to exit, then
  SIGKILL, and answers only then; a terminal's interactive login shell ignores SIGTERM, so every
  close took the whole grace (2.08 s measured on a dev Host). Sending SIGHUP first to a PTY run is
  a separate Host fix, and would still leave a round trip. The owner keeps the terminal until the
  close is confirmed (its stop, due again after a reconnect, and its place under the PTY limit
  unchanged); a start at the limit meanwhile waits for the close rather than show the limit, and a
  failed stop brings the tab back at the end, not active, with its retry.

## Packages

| | Package | Commits | State |
|---|---|---|---|
| F31 | Layout polish: the panel's corners, a 16 pt line, the diff in an inset box | b7b5cf4..cb7c9ab | done |
| F32 | Find in the conversation (⌘F), the `search` crate | 3efa0b2..6fca69e | done |
| F33 | Protocol: runtime resources, `recall.query`, `artifact.query` / `delete`, fixtures | b52c99b..5f1bbd1 | done |
| F34 | Search every task (⇧⌘F), a page in the plate | ec8a3a9..5de63cd | done |
| F35 | Terminal backend: Host-owned PTYs, the emulator, controls, input mapping | b30f22f..f076e5c | done |
| F36 | The workbar and the terminal view | 9a7aa3d..6fd00fa | done |
| F37 | Terminal finish: links, scrollbar, contrast, blink, Option-as-Meta setting | 598bc2e..deb46c1 | done |
| F38 | Files: the task's artifacts in the workbar (`artifact.query`) | d678a44..c27fa98 | done |
| F39 | Syntax colours in code blocks, diffs and files | 8e62b04..8b54453 | done |
| F40 | Trace: the task's trace, usage and context window (Desktop's Inspector) | 520a3a8..7cfda8e | done |
| F41 | Terminal fixes from an adversarial review: stuck attaches, the PTY count, re-acquire backoff, controllers only while the face shows | a7272f8..15184f5 | done |
| F42 | Side chat: a fork of the task in the workbar, its cleanup ledger, quotes from the transcript, hidden forks | c9fd75b..9adda9f | done |
| F43 | Files: HTML pages rendered in the face, links by Desktop's rule, pages opened in the default app | 4a05bc0..9b58c10 | done |
| F44 | Side chat fixes from an adversarial review: the ledger merged across runs and never overwritten unread, owner locks taken before they are named, `operation_conflict` settled, removals tried again, the picked mode set again | a754ada..cbbca54 | done |
| F45 | A State Root whose disk identity changed (macOS remounted its volume): Desktop's repair prompt in the disconnected strip, Repair workspace beside Retry, the marker's `rootIdentity` replaced under Maka's marker lock with before and after checks | e6d0a25..b5ec632 | done |
| F46 | File paths in replies open the file on a Host on this machine: previewed in Files, opened in the default app (documents only) or shown in Finder | bf7cd70..a527e4d | done |
| F47 | The prompt rail: a tick per prompt on the transcript's right edge that jumps to it, with prompts not held yet from the Host's Turn index | 6531dd0..9caff64 | done |
| F48 | A closed terminal's tab goes at once, the shell stopped behind it; a start at the PTY limit waits for the close | 8964432..4ce9811 | done |

Later candidates: Browser (gpui-kit has a `webview` crate; the model's browser tools need
client capabilities), and a webview for the Files face's HTML pages, which would run their
scripts and styles as Desktop's sandboxed frame does: gpui-kit's `gpui-webview` switches the
whole app to the GPUI Fast backend, too broad a change for one preview. Revisit with Browser.
For Side chat: the side composer's `/compact` once the main composer has it, quote notes
(Desktop's annotation panel) and the composer's optimistic message, an "Open side chat" palette
command with the other workbar tools, and a WorkHub coordination source when WorkHub arrives.

## Upstream

- `recall.query` indexes side-conversation forks with their task's copied words, and a fork's
  runs raise `session.catalog.changed` attentions like a task's: the client leaves both out
  (by the label, and by the forks its ledger holds). A Host-side filter would spare every
  client the same work.

- `runtime.resource.start` over the shared live-PTY cap fails in the shell-run manager, and the
  coordinator's `#resourceFailure` turns every error other than not-found into
  `internal_failure` and calls `#requestDrain()`
  (`packages/runtime-host/src/server/runtime-resource-coordinator.ts`, at the pinned commit): a
  ninth terminal restarts the Host. Recorded in `docs/upstream-issues.md` to fix ourselves.
- gpui-kit's diff tints changed rows at a fixed `success.opacity(0.12)` and has no single-column
  line numbers; both are upstream changes.
- gpui-kit's emphasis of the changed words inside a diff row is the status colour at a fixed 0.3
  over the row's 0.12 tint: on it the syntax colours fall under Lc 45 (to about Lc 26 on Mono
  light's removed rows), which the app cannot change.
- gpui-kit compiles a language's highlight queries each time it makes a highlighter, on the main
  thread for an editor: about 40 ms for Rust in the dev build each time the Files source view
  opens. A per-language cache of compiled queries would remove it.
- gpui-kit's code block highlighter hands tree-sitter the whole block as one edit, so a block
  growing in a streaming reply is parsed whole at each commit (only that block, never per
  frame); and a text view's first append after its synchronous first parse moves onto its
  background parser's copy of the blocks, which highlights each once more.
- tree-sitter-sequel (the SQL grammar) pins `cc ~1.2`, which holds the workspace at cc 1.2.67.
