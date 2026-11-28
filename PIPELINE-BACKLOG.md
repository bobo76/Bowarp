# Pipeline: next BACKLOG.md step → Rust review → verify & fix

## Context
Each run takes the first item under "Next steps" in `BACKLOG.md` (or under "## Backlog" when there is no "Next steps") and runs three agents in turn: implement and commit; review with the Rust checklist and the FM IPL rules into `REVIEW.md`; verify the findings, fix what's worth it and commit. Each agent logs its begin and end and writes a retrospective on the pipeline. UX/UI questions go to a UX/UI agent, whose decisions are recorded in `docs/ux-decisions.md`. A run's commits (a plan decision, the step, the review fixes, the docs update) are squashed into one and pushed. Runs follow one another until the list is empty (see "Loop").

Project rules (the check, where `cargo` is, BACKLOG/CHANGELOG conventions, UI-thread rules) are in `CLAUDE.md` and `crates/app/CLAUDE.md`, which every agent loads; this plan doesn't repeat them. Project facts (toolchain, library behaviour, test harness) are in project memory (see "Project memory").

## Pipeline files (`pipeline/`, git-ignored)
- `pipeline/log.txt`: append-only (`>>`, never edit earlier lines), one line per event across all runs: `<date -Iseconds> | <slug> | <agent> | <event> | <text>`. Events: `begin`, `end` (text says the outcome, e.g. `committed 8b77458, 109 tests` or `stopped: <reason>`), `usage`, `docs`, `split`, `restart`, `rebase`, `ci`, `ux`, `bump`; each is described where it is used.
- `pipeline/retros/<slug>/`: the run's retros.
- `pipeline/suggestions.md`: retro "What to change" items, one line each: `<suggestion> | runs: <slug>, <slug> | open|applied|rejected: <reason>`.
- `pipeline/tools/`: the Xvfb harness for driving the app. Agents use it as is and extend a script rather than write a one-off; usage and limits are in the Xvfb memory entry.

## Models
Always set the Agent tool's `model`, so a configured default can't downgrade an agent.
- Main session (me): the latest Opus (`claude-opus-5-5`). On another model, I ask the user to `/model opus` before Step 0.
- Agents 1 and 2, UX/UI agent: `model: "opus"`.
- Agent 3: `model: "sonnet"`, the only Sonnet agent.

## Project memory
In `~/.claude/projects/-home-bobo-dev-agent-workspace/memory/`. Agents only read it; I alone update it (see "After verification"). Entries and who gets them:
- `rust-toolchain-facts.md`: every agent.
- `xvfb-testing-limits.md`: agents that drive the app, and the UX/UI agent when it must see the UI.
- `gpui-ui-facts.md` (actions, focus, dialogs, menus, painting, drags, layout minimums, animation cost) and `gpui-windowing-facts.md` (window, platform, globals, fonts, async): when the step or diff touches that area. For a diff under ~100 lines, quote the few facts the reviewer needs instead.
- `copilot-cli-facts.md`, `claude-code-hook-facts.md`: steps touching agent integration.

## Retro focus
Retros are about **the pipeline, not the step**: how to make the next run faster and cheaper. Each answers:
- **Where time and tokens went** (big files read in full, repeated builds, harness setup, exploring code that didn't matter).
- **Waste** (redone work, context the prompt should have given, unneeded reads, failed commands).
- **What to change**: concrete edits to this plan or the prompts, each with its expected saving.
- **Step notes**: a few lines, only when they affect the pipeline. Product choices and untested items go in `BACKLOG.md` and the final report.

## Step 0 — me, before any agent
1. **Resume check, first:** a `pipeline | begin` line with no matching `pipeline | end` means the last run was interrupted. Take the slug and base from that line (agent 1's commit may already have removed the step from `BACKLOG.md`). Find the last agent with a `begin` and no `end`, and check git log, the working tree and `pipeline/retros/<slug>/`:
   - Its commit exists (agent 2: `REVIEW.md` is newer than its `begin` and its first line names this slug): run its gate, append `end | recovered: <outcome>`, continue with the next agent.
   - Uncommitted changes from it: ask whether to keep them (relaunch the agent to finish and commit) or discard them and rerun.
   - Nothing usable: append `pipeline | restart | <what was interrupted>` and rerun that agent.
   In every case skip the rest of Step 0; the gate covers the baseline.
2. **Dirty tree:** ask whether to commit the changes (with which message) or abort, before any commit of the run, so a split never sweeps the user's `BACKLOG.md` edits in.
3. **The step:** the first item (see Context); derive a short slug (e.g. `m2-part-e`). Nothing left: stop and report. Plainly not doable on this machine: mark it so (see "Loop") without launching an agent.
4. **Baseline:** `cargo build` and `cargo test`. Red: stop and report.
5. **Record the base:** `git rev-parse HEAD`, then append `pipeline | begin | <step text> (base <sha>)` before any commit of this run.
6. **Step size:** if the step bundles features that could ship alone, or would likely add more than ~1 500 lines, split it in `BACKLOG.md` into ordered sub-steps that each stand alone and can be tested, commit that (the run's plan decision commit), take the first sub-step, and append `pipeline | split | <first sub-step>` (the slug stays).
7. **UX/UI pre-check** (steps with UI): list the questions the step text, `FEATURES.md` and `docs/ux-decisions.md` leave open: where a control goes, its look and shortcut, edge cases, and unspecified behaviour (other keys a new focus target receives, how each exit path ends, edge states such as the Settings page shown). Run the UX/UI agent on them (see "UX/UI questions"; minor ones I decide) and pass its decisions to agent 1. In its prompt:
   - ask each decision to name the data it needs and whether the app can read it today (e.g. "exit code: the `D` mark carries none"), so new plumbing shows before agent 1 builds;
   - when the step assumes a capability that doesn't exist yet, say "probable split" and ask for the sub-step texts;
   - for an animation with no time limit, include its redraw cost (each frame re-renders the window; GPUI UI entry);
   - for a layout step (new pane, divider, fixed width), decide what happens at the smallest page (window minimum minus sidebar and explorer; GPUI UI entry), and give agent 1 the harness coordinates of the element to drive and the expected pixel numbers at the default geometry.

## Every agent prompt
- First command: append `begin` with the task; last command, after the commit and the retro: append `end` with the outcome.
- Stage files by path, never `git add -A`/`.`; leave files the agent didn't change alone. Never stage `pipeline/` or `REVIEW.md`; don't edit `PIPELINE-BACKLOG.md` (I commit it) or `CHANGELOG.md` (I write it at the squash).
- "Before anything else, read these memory files:" + absolute paths of the entries for this agent (see "Project memory"), never their text; don't edit them.
- Final report: the library/platform facts the code relies on, each with the `file:line` that confirms it, so the next agent checks those lines instead of searching.
- Run the toolchain memory's comment-width command right after each edit batch (and each doc edit), before the full test run.
- Run `cargo test --no-fail-fast` once (agent 2 only when it commits a fix), tee it to a scratch file named with the slug (the scratchpad is shared) and count with the toolchain memory's test-count command.
- Take the app's real shortcuts from `README.md`, not from guesses.
- Agent 2 gets agent 1's retro path and final report (what was verified and how, what wasn't driven, the choices made). Agent 3 gets agent 2's final report and any user or UX decisions, and refers to `REVIEW.md` finding numbers instead of pasted findings; no retros.

## Agent 1 — implement the step (general-purpose, opus, I wait for it)
Prompt contents:
- The step text from `BACKLOG.md`, verbatim, plus any reference screenshot it links to.
- Where to start, from a quick grep in Step 0: the key state fields and entry points (`file:line`); every call site of a signature the step changes (tests included); every exhaustive `match` over an enum it extends; other readers of the state it touches, and when that state is saved (`tabs.json` on each tab change, `settings.json` once per gesture).
- Read `FEATURES.md` when the step adds a switch ("Optional Behavior" rules and the "Settings" bullets), then the code the step touches.
- The `docs/ux-decisions.md` entries the step settles (including "Open" sub-bullets), so it resolves them there, and the UX/UI decisions from Step 0.
- Ambiguities (keys, edge cases, exact behaviour): pick the option consistent with the existing code and list it in the report. A new UX/UI question nothing settles: pick provisionally and list it under "Open UX questions" (question, choice, `file:line`), so it goes to the UX/UI agent before agent 2.
- Final report, besides the facts above:
  - behaviour changed on existing paths, apart from new-feature choices (that is where regressions hide);
  - the program input the feature can capture (keys, mouse, clipboard...) and whether it refuses, warns or allows each;
  - the `file:line` of each state change the feature relies on (where a field is set, cleared and read; which event triggers a redraw); for each new state flag, the event that ends it on every path (dropped entity included) and the redraw that follows;
  - where the keyboard goes after each changed path that moves focus;
  - for a feature switch, every gated path, start-up order included (settings load before tabs restore);
  - for a field added to a view-model struct, the existing test that builds it and whether the field is asserted.
- Not doable or not checkable here: stop before committing, revert its own changes, give the reason (see "Loop").
- Rules beyond `CLAUDE.md`: no `to_owned()` of text handed to a background task (move an `Arc` or cheap handle in); each new function one concern and ~15 lines (the FM IPL pass flags longer ones).
- Verify: clippy before the full test run (a lint-forced refactor after it costs a rerun). A popup or overlay: also test a dialog opened by a shortcut over it (where the keyboard goes when it closes). A change that replaces or hides a widget that can hold the keyboard: test where the keyboard goes. With UI: the touched crate's tests first, drive the app with `pipeline/tools/` (never set up a display by hand; say up front what the harness can't drive), then the full suite once.
- Docs: remove the step from `BACKLOG.md` and add the checks the harness couldn't drive under "Fixed, not tested yet"; new limitations go to `docs/known-limitations.md`; update `README.md` where it describes changed behaviour, and grep README, docs and code comments for stale wording.
- Commit code and those docs with an imperative summary of the step; write `pipeline/retros/<slug>/1-implement.md`.

## Agent 2 — Rust + FM IPL review (general-purpose, opus, after agent 1)
- Review `git show <sha>`, file by file (`git show <sha> -- <path>`, test files last): the changed code, the callers and callees it changes, and the state flow it relies on; targeted reads, not whole files, nothing older outside that. No toolchain pass: the gate already ran the check.
- **Checklist** (inline, instead of the `review-rust` skill, ~6k tokens, most of it irrelevant to a GPUI diff): ownership/clones in render paths; `Task` lifetimes (dropped = cancelled); redraw cost (what notifies, how often, while idle); state set/cleared on every path (exit, cancel, re-entry); persisted state the old view carried when a session is replaced (e.g. the conversation id in `tabs.json`); with focus or selection changes, every state where a widget holds focus shows it; bytes typed into the PTY; `expect`/`unwrap` on I/O; APIs newer than the MSRV; shell-semantics parity when a feature is widened to another shell (case, its own names and aliases, its privacy rules); test gaps. Load `review-rust` only for `unsafe`, FFI, async runtime or concurrency changes.
- **FM IPL pass** (skip under ~100 changed lines of Rust and say so in `REVIEW.md`), rules inline so it doesn't load `review-fm-ipl`: CC-1 names reveal intent (length proportional to scope; `i`, `id`, single-letter closure params fine); CC-2 one thing per function; CC-3 at most ~15 lines unless one thing at one level of abstraction; CC-9 no flag arguments (a GPUI builder helper whose `bool` only feeds `.when(flag)` is exempt unless it has two distinct bodies); CC-12 no negative conditionals (guard clauses fine); OC-1 one indentation level (not GPUI builder chains); OC-2 no `else`; OC-6 no abbreviations. Taste-only findings are "(optional)". Each cites its rule code.
- **Overwrite `REVIEW.md`**: first line `<!-- <slug> <reviewed sha> -->`. Findings numbered in one sequence, each with severity, `file:line`, problem, suggested fix (a near-ready diff when under ~10 lines), **the relevant lines quoted**, and `Source:` (`checklist` or `review-fm-ipl`).
- **Design choices first**, with the options and the existing API or code each would use (or "none"). Mark UX ones `UX`: they go to the UX/UI agent and on to agent 3, without stopping the loop. Decisions made before agent 2 (agent 1's open questions) go in its prompt as `ux-decision` findings to list. Any other design choice stops the loop before agent 3: append `pipeline | end | stopped: design choice needed on #<n>` and ask the user; on restart the resume check continues with agent 3, with the decision in its prompt.
- **No code changes or commit**, except: when every finding is Low, mechanical and has a ready diff (taste-only "(optional)" ones are left for the skip rule below, never applied) (or the only others are design choices), agent 2 applies them, runs `cargo fmt --all` and the check (comment-only fixes: `cargo fmt --check`, `cargo build` and the comment-width command), commits "Address Rust review findings", and marks each **Verdict:** Confirmed (self), **Status:** Fixed. Agent 3 is then skipped when nothing is left (saves ~40k tokens), or handles only the rest. When only "(optional)" findings remain, I skip agent 3 and mark them **Worth doing?** No, **Status:** Skipped. Otherwise doc- and comment-only fixes get a ready diff for agent 3 to apply without discussion.
- Write `pipeline/retros/<slug>/2-review.md`.

## Agent 3 — verify and fix (general-purpose, sonnet, after agent 2)
- For each finding, check the quoted lines and add **Verdict** (Confirmed / Refuted / Partly) and **Worth doing?** (Yes / No + reason), later **Status** (Fixed / Skipped). Decide "(optional)" ones from the quote alone; read more code only when the quote can't decide.
- Fix every "Yes" in one batch. Write each new test first and run it alone (`cargo test -p <crate> <path>`) before the full run. Then the check, until green; a fix stuck after 2 attempts is Skipped with the reason.
- Update `README.md`, `docs/known-limitations.md` and code comments whose wording a fix or decision changes, in the same commit.
- Commit "Address Rust review findings"; write `pipeline/retros/<slug>/3-fix.md`.

## UX/UI questions — UX/UI agent (`ux-designer`, opus, I wait for it)
Run it for a batch of questions (not one per question) from Step 0's pre-check, agent 1's "Open UX questions" (before agent 2) or `UX` findings in `REVIEW.md` (before agent 3). Its role, method and `docs/ux-decisions.md` entry format are in `.claude/agents/ux-designer.md`; don't repeat them. Prompt contents:
- Each question with its context: step text, slug, where it came up, the `file:line` of the code concerned (every line that writes the state in question), and edge facts it would otherwise grep for (whether a tab always exists, the real labels a key or collision rule depends on).
- The `README.md` lines describing the behaviour, so it names the ones its decisions make wrong; a `docs/known-limitations.md` line made wrong goes in its report for agent 3.
- The Xvfb entry path if it must see the UI, else "no need to see the UI live".
- Log lines and retro `pipeline/retros/<slug>/ux-<n>.md` (`<n>` counts this run's UX/UI agent runs). It doesn't commit; I commit `docs/ux-decisions.md` with the docs.

I adopt its recommendations: append `ux-agent | ux | <question> -> <decision>` per question and pass them on (to agent 1 from Step 0; to agent 3 from agent 1's report or `REVIEW.md`, as a fix where it differs from what was built). When it says a question needs the user (it contradicts `FEATURES.md`, or the options are equally good and it is product direction), I stop the loop like a design choice, show its options, and record the user's pick in the entry as `[user]`.

**Minor UX questions** (wording, placement or styling following an existing pattern; a review choice whose options are all local to the step) I decide myself, anywhere: append `pipeline | ux | <question> -> <decision>`, add a short `[pipeline]` entry to `docs/ux-decisions.md` when the code doesn't make the choice visible, and pass it on. The UX/UI agent is for new layouts, new interaction flows, unclear edge states, or anything touching `FEATURES.md`.

## Gate between agents
Before the next agent I check:
- the expected commit exists (agent 1: the step; agent 3: the fixes), or for agent 2 that `REVIEW.md` was rewritten with no code changes in the tree (or its fixes commit exists);
- the check is green, with `cargo test --no-fail-fast` teed so a failing test is named before any rerun (one that fails once then passes is logged as flaky and named to agent 2); skipped when HEAD hasn't moved since the last gate;
- the agent's retro exists, and the log has its `begin` and `end`;
- after a UX/UI agent: a short `docs/ux-decisions.md` entry and a `ux` log line per question.

Then I append `<agent> | usage | <tokens> tokens, <tool uses> tool uses, <duration>` from its completion notice. Any failed check (including agent 1 unable to run a needed UI check): append `pipeline | end | stopped: <reason>` and report instead of going on.

**Windows CI before review** (the step changes code that only runs on Windows or under PowerShell): after agent 1's gate (and again after agent 3's), `git push origin HEAD:refs/heads/ci/<slug>`, `gh workflow run windows.yml --ref ci/<slug>`, watch it in the background while agent 2 runs, then `git push origin --delete ci/<slug>`. Red: pass the failing job and test to agent 3 as a finding (or stop if it needs a design choice); green: tell agent 3. Tell such agents that downloading tools (pwsh) is blocked.

## After all agents
- **Docs**, from the agents' reports, committed alone as "Update docs after <slug>" if anything changed:
  - `FEATURES.md`: add or reword the line for user-visible behaviour it doesn't describe (a spec, not a changelog: no step names, versions or paths; nothing for internal work).
  - `BACKLOG.md`: remove what the run completed (items covered, or their done part; "Fixed, not tested yet" entries resolved); remove `docs/known-limitations.md` entries it lifted. Don't touch the rest.
  - `docs/ux-decisions.md`: the UX/UI agent's entries of this run.
- **Squash and push** (`<base>..HEAD`):
  - Every commit there must belong to this run; another one (the user's) mid-run: stop and ask.
  - `git fetch origin`; if the remote branch moved, `git rebase origin/<branch>` (conflicts: keep both sides, the user's `BACKLOG.md` edits win minus the step just done), set `<base>` to `origin/<branch>`, append `pipeline | rebase | new base <sha>` and re-run the check.
  - `CHANGELOG.md`: add the step's user-visible changes under `## Unreleased`, in the file's style (short bullets starting with a verb, wrapped near 90 columns, settings and shortcuts as the user sees them, no paths or step names; behaviour changes to existing paths too). Nothing for a change no user would notice.
  - `git reset --soft <base>`, `git add CHANGELOG.md`, commit with agent 1's message plus a short body listing the review fixes; re-run the check.
  - `git push origin <branch>` (`-u` without upstream). Never force-push; rejected: stop and report.
  - No version bump in a run: one bump closes the loop (see "Version bump at the end").
  - **CI**: workflows don't run on branch pushes: `gh workflow run ubuntu.yml --ref <branch>` and `windows.yml`, find each run with `gh run list --workflow <file> --branch <branch> --limit 1 --json databaseId,headSha` (`headSha` = the squashed commit), `gh run watch <id> --exit-status` in the background. Append `pipeline | ci | ubuntu <conclusion>, windows <conclusion>`. Any red (Ubuntu's Clippy job included): stop the loop, append `pipeline | end | stopped: CI <workflow> failed (<run url>)`, report the failing job.
- Append `pipeline | end | <squashed sha> pushed, <test count>` (counted with the toolchain memory's command on the teed output).
- Report: build status, the pushed commit, test count, `REVIEW.md` summary (confirmed, fixed, skipped), each agent's tokens and duration, the top retro suggestions, links to the retros.

## Verification
- Every gate passed (logged); the check and both CI workflows are green on the squashed commit (`ci` line).
- `git log <base>..HEAD` shows one commit, and `git status -sb` shows the branch not ahead of its upstream.
- `CHANGELOG.md` lists the step under Unreleased (unless nothing user-visible changed).
- `REVIEW.md` has a verdict and a status on every finding.
- The log has begin, end and usage for each agent and begin/end for the pipeline.
- `pipeline/retros/<slug>/` holds 3 files (2 when agent 2 applied the fixes) plus one `ux-<n>.md` per UX/UI run; every UX/UI decision is in `docs/ux-decisions.md` in the squashed commit; `pipeline/` is in neither `git status` nor `git log`; the commit doesn't touch `PIPELINE-BACKLOG.md`.
- The step is gone from `BACKLOG.md`.

## After verification — memory and pipeline (me)
- **Project memory:** save the library/platform facts and harness limits the run confirmed (from reports and retros). Update an entry rather than duplicate it, fix or delete entries the run proved wrong, keep `MEMORY.md` in sync. Skip when nothing new was learned.
- **Suggestions:** merge the retros' "What to change" items into `pipeline/suggestions.md` (add the slug to a matching line's runs, or add an `open` line). Apply every `open` one with 2+ runs to this plan or `pipeline/tools/` and mark it `applied`, or `rejected: <reason>`; apply a single-run one at once only when it is a clear win (a missing prompt fact, a broken command). If `PIPELINE-BACKLOG.md` changed, commit it alone ("Update pipeline backlog after <slug>") and push it.
- Append `pipeline | docs | FEATURES: <changes or "no changes">, memory: <entries or "none">, plan: <suggestions applied or "none">`.

## Loop until BACKLOG.md is done
After Verification and the memory step, start the next run from Step 0 without waiting for the user, until the list is empty. Then append `all | pipeline | end | plan done, <runs> runs, <test count>` and give a final report: one line per run (slug, commit, test count, findings fixed/skipped, tokens, duration), the steps marked not doable with reasons, the suggestions applied or rejected during the loop, and the version bump (or why there was none). Each run still gives its own short report and memory update, so a later stop loses nothing.

**Step not doable** (needs Windows, a real device, a decision the plan doesn't give), seen in Step 0 or reported by agent 1 (which reverts its own uncommitted changes): move the step to a "Not doable yet" section in `BACKLOG.md` (after "Next steps", created if missing) with a one-line reason, commit it alone "Mark <step> as not doable yet" (squashing in this run's split commit, if any), push, append `pipeline | end | not doable: <reason>` and start the next run.

The loop stops on anything else that stops a run: a red baseline, gate or CI, a failed Verification check, uncommitted changes Step 0 can't explain, or a design choice or UX/UI question that needs the user. I report why and which steps are left.

### Version bump at the end
One bump covers all the runs of a launch ("do the next 3 items", "until BACKLOG.md is done"), after the last run's memory step and before the final report:
- **When:** the launch asked for a number of runs or for the whole list, and the loop reached it: bump without asking. In any other case (the launch didn't say, the loop stopped early, usage limits cut it): ask the user "Bump the version for these <n> runs?" (Yes patch / Yes minor / No) and wait. No bump when no run pushed a user-visible change (`## Unreleased` empty), or when the user says No.
- **Level:** `patch`; `minor` when a run finished a milestone (M2 done -> `0.2.0`).
- **How:** `git fetch origin` (rebase as in "Squash and push" if the branch moved); `scripts/bump-version.sh [minor]` on a clean tree (it refuses a dirty `Cargo.toml`; it prints the new version); in `CHANGELOG.md` move the Unreleased items into a new `## <version>` section just below `## Unreleased`, leaving Unreleased empty; commit `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md` as "Bump version to <version>"; run the check; push; run both CI workflows on it as in "Squash and push".
- Append `all | pipeline | bump | <version> (<sha>), CI ubuntu <conclusion>, windows <conclusion>`, or `all | pipeline | bump | skipped: <reason>`.
- Merging to `main`, the `v<version>` tag and the GitHub release follow `RELEASE.md` and are not part of the pipeline: I name them in the final report as the next step.
