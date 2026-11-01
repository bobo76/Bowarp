# Pipeline: next BACKLOG.md step → Rust review → verify & fix

## Context
A reusable pipeline: each run takes the first item under "Next steps" in `BACKLOG.md` and runs three agents one after another: implement and commit, review with the Rust checklist and the `review-fm-ipl` skill into `REVIEW.md`, then verify the findings, fix what's worth it and commit. Each agent logs when it starts and ends, and writes a retrospective on the pipeline itself when it's done. UX/UI questions that come up along the way go to a UX/UI agent, which investigates them and recommends a solution; each question, its options with pros and cons, and the decision are saved in `docs/ux-decisions.md` (see "UX/UI questions"). When a run is done, its commits (up to ~4: a plan decision, the step, the review fixes, the docs update) are squashed into one and pushed. Runs follow one another until "Next steps" is empty (see "Loop until BACKLOG.md is done").

## Pipeline files (git-ignored)
Everything the pipeline produces about itself lives in `pipeline/` at the repo root, which is listed in `.gitignore`:
- `pipeline/log.txt`: one line per event, appended and never rewritten, across all runs:
  `<ISO time> | <slug> | <agent> | begin|end | <task in one line>`
  On `end`, the task part also says the outcome, e.g. `committed 8b77458, 109 tests` or `stopped: test display unavailable`.
- `pipeline/retros/<slug>/`: the 3 retros of a run.
- `pipeline/suggestions.md`: the retros' "What to change" items across runs, one line each: `<suggestion> | runs: <slug>, <slug> | open|applied|rejected: <reason>` (see "After verification").

**The check** (used by the gate, the squash and Verification): `cargo fmt --check && cargo build && cargo test && cargo clippy --all-targets -- -D warnings`. Where `cargo` lives (it may not be on PATH) is in the toolchain memory entry.

Get the time with `date -Iseconds`. Append to the log with `>>` and never edit earlier lines.

Event types: `begin`, `end`, `usage`, `docs` (see "After verification"), `split` (the step was split; see Step 0), `restart` (an agent run again after an interruption; see Step 0), `rebase` and `ci` (see "Squash and push"), `ux` (a UX/UI question decided; see "UX/UI questions").

## Models
- **Main session (me):** the latest Opus (currently Opus 5.5, `claude-opus-5-5`). If the session runs on another model, I stop and ask the user to switch (`/model opus`) before Step 0.
- **Agents 1 and 2:** the latest Opus. Launch them with the Agent tool's `model` set to `opus`; never leave it unset, so a configured default subagent model can't downgrade them.
- **Agent 3 (verify and fix):** Sonnet, `model: "sonnet"`. It is the only agent that runs on Sonnet.
- **UX/UI agent:** the latest Opus, `model: "opus"` (also set in its agent file).

## UI test tools (kept, git-ignored)
`pipeline/tools/` holds the harness for driving the app in a headless display. Agents use it as is and do not rebuild it; if a tool is missing something a step needs, extend the script rather than writing a one-off. What the scripts are, how to call them and what they can't drive is in project memory, not here.

## Project knowledge (project memory)
Everything specific to the project (toolchain constraints, test harness usage and limits, confirmed library behaviour) lives in project memory, so this plan stays generic. The one exception is agent 2's review checklist, which names the project's stack on purpose: it replaces the generic `review-rust` skill. Each prompt lists the absolute paths of the entries the agent must read first (see "Every agent prompt"). Memory lives in `~/.claude/projects/-home-bobo-dev-agent-workspace/memory/`. Agents only read it; I am the only one who updates it (see "After verification").

## Retro focus
The retros are about **the pipeline, not the step**: how to make the next run faster and use fewer tokens. Each retro answers:
- **Where time and tokens went:** the steps that cost the most, e.g. reading big files in full, repeated builds, test harness setup, exploring code that turned out not to matter.
- **Waste:** work that was redone, context the prompt should have given but didn't, files read but not needed, commands that failed and had to be retried.
- **What to change:** concrete edits to this plan or the agent prompts (context to pass in, files to skip, scripts or helpers to keep, checks to merge or drop), each with the expected saving.
- **Step notes:** at most a few lines about the step itself, and only when they affect the pipeline. Product choices and what's untested go in `BACKLOG.md` and the final report, not here.

## Step 0 — me, before any agent
In this order:
1. **Resume check, first:** look in `pipeline/log.txt` for a `pipeline | begin` line with no matching `pipeline | end` for the same slug. If one exists, the last run was interrupted: take the slug and base from that line, not from "Next steps", because agent 1's commit may already have removed the step from it. Then find the last agent with a `begin` and no `end`, and look at git log, the working tree and `pipeline/retros/<slug>/` to see how far it got:
   - The agent's commit exists (for agent 2: `REVIEW.md` is newer than agent 2's `begin` time and its first line names this slug): run its gate, write the missing `end` line (`end | recovered: <outcome>`), and continue with the next agent.
   - Uncommitted changes from the interrupted agent: I stop and ask whether to keep them (relaunch the agent telling it to finish and commit them) or discard them and rerun.
   - Nothing usable: append `<time> | <slug> | pipeline | restart | <what was interrupted>` and rerun that agent.
   In every case, skip the rest of Step 0; the gate covers the baseline.
2. **Dirty tree:** if the working tree has uncommitted changes, I stop and ask whether to commit them (and with which message) or abort. This comes before any commit of the run, so a split never sweeps the user's `BACKLOG.md` edits into it.
3. Read the first item under "Next steps" in `BACKLOG.md` (or, when there is no "Next steps"
   section, the first item under "## Backlog"); that is **the step** for this run. Derive a short slug from it (e.g. `m2-part-e`) for the retro folder. If both are empty, I stop and report. If the step plainly can't be done on this machine, I mark it not doable and move on (see "Loop until BACKLOG.md is done") without launching any agent.
4. Run `cargo build` and `cargo test`, so we know the baseline is green. If it's red, I stop and report back.
5. **Record the base:** `git rev-parse HEAD`, then append `<time> | <slug> | pipeline | begin | <step text> (base <sha>)` to `pipeline/log.txt`, before any commit of this run, so a resumed run can find it.
6. **Step size check:** if the step bundles several features that could each ship alone (e.g. a protocol, a store and two UI pieces), or would likely add more than ~1 500 lines, split it in `BACKLOG.md` into ordered sub-steps that each stand alone and can be tested, and commit that as the run's plan decision commit. Then take the first sub-step as the step, and append `<time> | <slug> | pipeline | split | <first sub-step>` (the slug stays the same for the run).
7. **UX/UI pre-check:** if the step has UI, list the UX/UI questions its text, `FEATURES.md` and `docs/ux-decisions.md` leave open (where a control goes, what it looks like, its shortcut, what the user sees on an edge case), including behaviours the step leaves unspecified (the other keys a new focus target receives, how each exit path ends, edge states such as the Settings page shown), so they are decided before agent 1 builds. When the step assumes a capability that doesn't exist yet, say "probable split" in the UX prompt and ask for the sub-step texts. If there are any, run the UX/UI agent on them (see "UX/UI questions") and pass its decisions to agent 1. In the UX prompt, ask that each decision name the data it needs (an exit code, a title, a state) and whether the app can read it today (e.g. "exit code: the `D` mark carries none"), so a decision that needs new plumbing is visible before agent 1 builds. Minor questions with an obvious answer from the existing UI don't need it. For an animation with no time limit, include its redraw cost in the question (each frame re-renders the whole window; see the GPUI memory entry).

## Every agent prompt
- First command: append `begin` with the agent's task to `pipeline/log.txt`. Last command, after the commit and the retro: append `end` with the outcome.
- Stage files by path; never `git add -A` or `git add .`. Leave uncommitted files that the agent didn't change alone (they may be the user's notes).
- Never stage `pipeline/` or `REVIEW.md` (untracked, not git-ignored), and don't edit or stage `PIPELINE-BACKLOG.md` (it is tracked, but only I change and commit it; see "After verification"). Don't edit `CHANGELOG.md`; I write it when the version is bumped.
- "Before anything else, read these memory files:" followed by the absolute paths of the entries marked for every agent (toolchain facts) and of those relevant to the step (library behaviour, test harness usage and limits). Paths only, never the text of the entries. Tell the agent not to edit them. Pick per agent: the Xvfb entry only for agents that drive the app, the GPUI entry only when the diff touches rendering, windows, actions or dialogs (agent 2 on model-only code gets toolchain facts alone).
- Ask for a final report that lists the library/platform facts the code relies on, each with the source `file:line` that confirms it, so the next agent checks those lines instead of searching again. For agent 1 this also includes the `file:line` of each state change the feature relies on (where a field is set, cleared and read, which event triggers a redraw), so the reviewer can follow the state flow without a grep round.
- For agent 2: the path of agent 1's retro and agent 1's final report (what was verified and how, what was not driven, the choices made). Read those before the code instead of working them out again.
- For agent 3: agent 2's final report and any user decisions; point to `REVIEW.md` for the findings instead of pasting them, and use REVIEW.md's finding numbers. No retros needed.
- Tell agents to run the toolchain memory's comment-width command before committing.
- Tell agents to run `cargo test --no-fail-fast`, tee its output to a scratch file named with the slug (the scratchpad is shared) and count from it with the toolchain memory's test-count command (it skips filtered reruns), not run the tests twice, and to take the app's real shortcuts from project memory rather than guessing them.

## Agent 1 — implement the step (general-purpose, `model: "opus"`, I wait for it)
Launch this agent with the Agent tool's `model` set to `opus`.
Prompt contents:
- The step text from `BACKLOG.md`, verbatim.
- Where to start: the key state fields and entry points (`file:line`) I found with a quick grep in Step 0, so the agent skips the search; every call site of a signature the step will change (tests included), other readers of the state it touches, and when that state is saved (e.g. `tabs.json` only at window close).
- Read `BACKLOG.md` (the step, plus any reference screenshot it links to) and `FEATURES.md` (including "Optional Behavior" when the step adds a switch), then the code the step touches.
- The `docs/ux-decisions.md` entries the step settles (including "Open" sub-bullets), found with a grep in Step 0, so the agent resolves them there.
- Where the step is ambiguous (key bindings, edge cases, exact behaviour), pick a sensible option consistent with the existing code and list the choice in its final report. Follow `docs/ux-decisions.md` and the UX/UI decisions given in the prompt. A new UX/UI question that neither settles: pick provisionally and list it under "Open UX questions" in the final report (question, the choice made, `file:line`), so it goes to the UX/UI agent before agent 2.
- In the final report, list behaviour that changed for existing paths (e.g. tab close no longer keeps viewer edits) separately from new-feature choices: that is where regressions hide. Also name the kinds of program input the feature can capture (keys, mouse, clipboard...) and whether it refuses, warns or allows each. When a changed path moves keyboard focus, say where the keyboard goes after each one (with the `file:line` that sets it). For each new state flag, say which event ends it on every path (including a dropped entity) and which redraw follows.
- If the step can't be done or checked on this machine, stop before committing, revert your own changes and give the reason (see "Step not doable" under the loop section).
- Rules:
  - Follow the existing code style.
  - Put pure logic in a non-UI crate with unit tests.
- Verify with `cargo fmt`, `cargo clippy --all-targets`, `cargo build` and `cargo test`; when the step has UI, run only the touched crate's tests first, drive the app, then run the full suite once (a fix found in the app then costs no second full run). If the step has UI, drive the app with the scripts in `pipeline/tools/` (see "UI test tools"); don't set up the display by hand. Say up front which parts the harness can't drive.
- Update `BACKLOG.md`: remove the step from "Next steps" and add the checks the harness couldn't drive under "Fixed, not tested yet" (the rest of the cleanup happens after Verification). Add any new known limitation to `docs/known-limitations.md`. No test counts or verification records in `BACKLOG.md`; the test count goes in the `end` log line.
- Update `README.md` where it describes the behaviour the step changes (not `CHANGELOG.md`).
- Commit the code, `BACKLOG.md`, `README.md` and `docs/known-limitations.md` (if changed) with an imperative summary of the step as the message.
- Write `pipeline/retros/<slug>/1-implement.md` (see Retro focus).

## Agent 2 — Rust + FM IPL review (general-purpose, `model: "opus"`, after agent 1 finishes)
Launch this agent with the Agent tool's `model` set to `opus`.
- Review agent 1's commit with a short project checklist in the prompt instead of loading the full `review-rust` skill (most of its sections don't apply to a GPUI diff, and it costs ~6k tokens). The checklist: ownership/clones in render paths, `Task` lifetimes (dropped = cancelled), redraw cost (what notifies, how often, while idle), state set/cleared on every path (exit, cancel, re-entry), when the step touches focus or selection: every state where a widget holds focus shows it, bytes typed into the PTY, error handling (`expect`/`unwrap` on I/O), APIs newer than the MSRV (see the toolchain memory entry), and test gaps. Load the skill only for `unsafe`, FFI, async runtime or concurrency changes. Review (`git show <sha>`): the changed code, the callers and callees it changes, and the state flow it relies on. Don't review older code outside that, and don't read whole files when the diff and a targeted read will do.
- **Second pass, FM IPL standards** (skip it when the diff changes fewer than ~100 lines of Rust; say so in `REVIEW.md`): review the same diff against these rules from the `review-fm-ipl` skill, given inline in the prompt so the agent doesn't load the skill (~2-3k tokens): CC-1 names reveal intent (length proportional to scope; `i`, `id`, single-letter closure params are fine), CC-2 a function does one thing (no "and"), CC-3 functions at most ~15 lines unless one thing at one level of abstraction, CC-9 no flag arguments (positional `bool` = two functions; a GPUI builder helper whose `bool` only feeds `.when(flag)` is exempt unless it has two distinct bodies), CC-12 avoid negative conditionals (guard clauses fine), OC-1 one level of indentation per function (not GPUI element builder chains), OC-2 no `else` (guard clauses, early returns), OC-6 don't abbreviate. Mark taste-only findings "(optional)". Every finding from this pass cites its rule code (e.g. `CC-3`, `OC-5`) and goes into the same `REVIEW.md`, numbered in sequence with the checklist findings, with a `Source:` line (`checklist` or `review-fm-ipl`). Same scope as the checklist pass: only code the diff adds or changes.
- Skip any toolchain pass (also when the skill is loaded): the gate already ran fmt, clippy and test on this commit and they're green. Findings come from reading the code.
- **Overwrite** `REVIEW.md` with the findings. Its first line is `<!-- <slug> <reviewed sha> -->`, so a resumed run can tell it belongs to this run. Each finding gets a number, a severity, `file:line`, the problem, a suggested fix, and **the relevant lines quoted** (a few lines, enough for agent 3 to check the finding without searching for the code). When the fix is under ~10 lines, give it as a near-ready diff.
- List findings that need a design choice first in `REVIEW.md`. Flag them with the options, and name the existing API or code each option would use (or say there is none), so agent 3 doesn't explore for it. Mark a design choice that is about UX/UI (what the user sees or does, not code structure) with `UX`. UX choices go to the UX/UI agent (see "UX/UI questions") and its decisions are passed to agent 3 (decisions made before agent 2, e.g. from agent 1's open questions, go in agent 2's prompt as `ux-decision` findings to list, so they are applied with the rest); the loop doesn't stop for them. For any other design choice, I stop the loop before launching agent 3 (append `<time> | <slug> | pipeline | end | stopped: design choice needed on #<n>`) and ask the user to pick. When the user restarts, the resume check continues with agent 3 and I pass the decision in its prompt.
- Code changes: none. No commit. Exception: when every finding is Low, mechanical and has a ready diff (comment wrap, typo, rename), or when the only others need a design choice, agent 2 applies the mechanical ones, runs `cargo fmt --all` and then the check, commits "Address Rust review findings", marks each finding **Verdict:** Confirmed (self) and **Status:** Fixed in `REVIEW.md`, and agent 3 is skipped when nothing else is left (saves a ~40k-token agent); otherwise agent 3 handles only the rest. When the only findings left are "(optional)", I skip agent 3 too and mark them **Worth doing?** No, **Status:** Skipped myself. Otherwise agent 2 still leaves doc-comment and comment-only fixes with a ready diff for agent 3 to apply without a verdict discussion.
- Write `pipeline/retros/<slug>/2-review.md` (see Retro focus).

## Agent 3 — verify and fix (general-purpose, `model: "sonnet"`, after agent 2 finishes)
Launch this agent with the Agent tool's `model` set to `sonnet`.
- For each finding in `REVIEW.md`, check the quoted lines and confirm or refute it; for an "(optional)" finding decide from the quote alone, without reading more code. Read more code only when the quote isn't enough to decide (e.g. whether a state is cleared elsewhere). Next to each problem, add a **Verdict** (Confirmed / Refuted / Partly), a **Worth doing?** (Yes / No + one-line reason) and later a **Status** (Fixed / Skipped).
- Fix every finding marked "Yes", all in one batch, then run fmt, clippy, build and test. Iterate until it's green. If a fix is stuck after 2 attempts, mark it Skipped with the reason.
- Commit the fixes (`REVIEW.md` stays local and untracked): "Address Rust review findings".
- Write `pipeline/retros/<slug>/3-fix.md` (see Retro focus).

## UX/UI questions — UX/UI agent (`ux-designer`, `model: "opus"`, I wait for it)
Launch it with the Agent tool's `subagent_type` set to `ux-designer` and `model` set to `opus`, whenever a UX/UI question comes up: in Step 0's pre-check, under "Open UX questions" in agent 1's report (run it before agent 2), or as a `UX` design-choice finding in `REVIEW.md` (run it before agent 3). One agent per batch of questions, not one per question.
Its role, what it reads, how it weighs options (2-3 per question, with pros and cons), the `docs/ux-decisions.md` entry format and its final report are in `.claude/agents/ux-designer.md`; don't repeat them in the prompt. If that agent type isn't listed (the session started before the file existed), launch a general-purpose agent with `model: "opus"` and tell it to read and follow that file first.
Prompt contents:
- The questions, each with its context: the step text, the slug, where it came up (Step 0, agent 1's report, `REVIEW.md` #<n>) and the `file:line` of the code concerned, if any, including every line that writes the state in question, and edge facts it would otherwise grep for (e.g. whether a tab always exists, the real labels or names a key or collision rule depends on).
- The `README.md` lines describing the behaviour in question, so it names the ones its decisions make wrong.
- The Xvfb memory entry path, if it needs to see the current UI.
- The log lines and the retro (`pipeline/retros/<slug>/ux-<n>.md`, `<n>` counting the UX/UI agent runs of this run), as in "Every agent prompt".
- Don't commit; I commit `docs/ux-decisions.md` with the docs (see "After all agents"). If a decision makes a `docs/known-limitations.md` line wrong, name the line in the report; agent 3 updates it. Say "no need to see the UI live" when the question is about behaviour or timing, not layout.

I adopt the recommendation and continue: append `<time> | <slug> | ux-agent | ux | <question> -> <decision>` to `pipeline/log.txt` per question, then pass the decisions on (to agent 1 from Step 0; to agent 3 from agent 1's report or `REVIEW.md`, as a fix to apply where it differs from what agent 1 built). Exception: when the agent says a question needs the user (it changes or contradicts `FEATURES.md`, or the options are equally good and the choice is a matter of product direction), I stop the loop like a non-UX design choice, show the user the options from the agent's report, and record their pick in its entry (`[user]`).

## Gate between agents
Before launching the next agent, I check that:
- the expected commit exists (agent 1: the step's commit; agent 3: review fixes commit), or for agent 2 that `REVIEW.md` was rewritten and the working tree has no code changes (or, when agent 2 applied the fixes itself, its review fixes commit exists and agent 3 is skipped);
- the check (see "Pipeline files") is green, run with `cargo test --no-fail-fast` teed to a scratch file so a failing test is named before any rerun (a test that fails once and passes on reruns is logged as flaky and named to agent 2); skip it when HEAD hasn't moved since the last gate (agent 2 with no fixes), because that commit was already checked;
- the agent's retro exists in `pipeline/retros/<slug>/`;
- `pipeline/log.txt` has the agent's `begin` and `end` lines.
- after a UX/UI agent: `docs/ux-decisions.md` has a short entry per question (see the rule at its top), and the log has a `ux` line per question.

**Windows CI before review** (when the step changes code that only runs on Windows or under PowerShell, which this machine can't run): after agent 1's gate (and again after agent 3's), push HEAD to a temporary branch (`git push origin HEAD:refs/heads/ci/<slug>`), start `gh workflow run windows.yml --ref ci/<slug>`, wait for it in the background while the UX/UI agent runs, give the reviewer the result (and the failing test, if any), then delete the branch (`git push origin --delete ci/<slug>`). The run's commits stay unpushed on `<branch>`, so the squash is unaffected. Tell such agents that downloading tools (e.g. pwsh) is blocked.

Then I append `<time> | <slug> | <agent> | usage | <tokens> tokens, <tool uses> tool uses, <duration>` to `pipeline/log.txt`, from the agent's completion notice, so the retros of later runs can compare costs.

If any check fails, including agent 1 being unable to run a needed UI check, I stop the pipeline, append `<time> | <slug> | pipeline | end | stopped: <reason>`, and report what failed instead of launching the next agent.

## After all agents
- **Docs:** bring the docs in line with what shipped, from the agents' final reports, so they go out in the same squashed commit:
  - **`FEATURES.md`:** if the step added or changed user-visible behaviour that no line describes (or describes differently), add or reword the line in the matching section. It stays a spec of the product, not a changelog: no step names, versions or file paths. Nothing to change for internal work (refactors, fixes of behaviour already described).
  - **Clean `BACKLOG.md` of what's done:** finished work lives in git history, not here. Remove what this run completed: backlog items the step covered (trim an item to its unfinished part when only part of it is done) and "Fixed, not tested yet" entries the step resolved. Also remove entries in `docs/known-limitations.md` the step lifted. Don't touch items the run didn't cover.
  - **`docs/ux-decisions.md`:** the entries the UX/UI agent added this run.
  - If any of these files changed, commit them alone: "Update docs after <slug>".
- **Squash and push:** the run's commits are `<base>..HEAD` (usually 2-4: a `BACKLOG.md` decision commit if any, the step, the review fixes, the docs update).
  - Check first that every commit in `git log <base>..HEAD` belongs to this run. If another one landed mid-run (e.g. the user's own commit), I stop and ask instead of squashing it in.
  - **Sync with the remote before the bump:** `git fetch origin`. If the remote moved (the user commits mid-run), `git rebase origin/<branch>` the run's commits, resolve conflicts keeping both sides (the user's BACKLOG.md edits win, minus the step just done), set `<base>` to `origin/<branch>` (log `<time> | <slug> | pipeline | rebase | new base <sha>`) and re-run the check. Doing this before the bump means a version bump or CHANGELOG edit by the user is picked up instead of conflicting with ours.
  - Bump the version while the tree is still clean: `scripts/bump-version.sh` (patch), or `scripts/bump-version.sh minor` when the step finishes a milestone (M2 done -> `0.2.0`). It prints the new version; it refuses a dirty `Cargo.toml`, which is why it runs before the reset.
  - **`CHANGELOG.md`:** right after the bump, add a `## <version>` section just below `## Unreleased`, moving any items listed under Unreleased into it. Write the step's user-visible changes from the agents' final reports, in the file's style: short bullets starting with a verb (Add, Show, Keep...), wrapped near 90 columns, naming settings and shortcuts as the user sees them, no file paths, step names or internal details. Mention behaviour changes to existing paths too. Leave `## Unreleased` empty. If the step changed nothing a user would notice (refactor, tests, internal fix), add no section; the version simply has no entry.
  - `git reset --soft <base>`, `git add Cargo.toml Cargo.lock CHANGELOG.md`, then `git commit` with the step's commit message (agent 1's), plus a short body listing the review fixes. Re-run the check on the squashed commit.
  - `git tag -a v<version> -m v<version>` on the squashed commit (local only for now).
  - `git push origin <branch>` (add `-u` if the branch has no upstream). Never force-push: the run's commits were never pushed, so the squash only rewrites local history. If the push is rejected (the remote moved in the meantime), I stop and report instead of rebasing or forcing.
  - **CI before the tag** (RELEASE.md: a pushed `v*` tag publishes a GitHub release, so it must only go on a green commit). The workflows don't run on branch pushes, so start them: `gh workflow run ubuntu.yml --ref <branch>` and `gh workflow run windows.yml --ref <branch>`, find each run with `gh run list --workflow <file> --branch <branch> --limit 1 --json databaseId,headSha` (its `headSha` must be the squashed commit), and wait with `gh run watch <id> --exit-status` (run in the background; I'm notified when it exits). Log `<time> | <slug> | pipeline | ci | ubuntu <conclusion>, windows <conclusion>`.
  - Both green (Ubuntu's separate Clippy job included): `git push origin v<version>`. Any red: stop the loop, keep the tag local, append `<time> | <slug> | pipeline | end | stopped: CI <workflow> failed (<run url>)`, and report the failing job.
- Append `<time> | <slug> | pipeline | end | <squashed sha> pushed, <test count>` to `pipeline/log.txt`.
- Report the build status, the squashed and pushed commit, the test counts, a summary of `REVIEW.md` (confirmed, fixed and skipped counts), each agent's tokens and duration, the top speed/token suggestions from the 3 retros, and links to the retros.

## Verification
- Every gate passed (logged), and the check is green on the squashed commit.
- `git log <base>..HEAD` (with `<base>` moved if the run was rebased) shows exactly one commit (the squashed step), and `git status -sb` shows the branch is not ahead of its upstream (pushed).
- Both CI workflows passed on the squashed commit (logged `ci` line), it raises the workspace `version` in `Cargo.toml`, is tagged `v<version>`, and `git ls-remote --tags origin v<version>` finds the tag.
- `CHANGELOG.md` has a `## <version>` section for the new version (unless the step had no user-visible change), and `## Unreleased` is empty.
- `REVIEW.md` has a verdict and a status on every finding.
- `pipeline/log.txt` has begin, end and usage lines for each agent, and begin/end for the pipeline.
- `pipeline/retros/<slug>/` holds 3 files (2 when agent 2 applied the fixes) plus one `ux-<n>.md` per UX/UI agent run, every UX/UI decision of the run is in `docs/ux-decisions.md` and in the squashed commit, `pipeline/` doesn't show up in `git status` or `git log`, and the squashed commit doesn't touch `PIPELINE-BACKLOG.md`.
- The step is gone from "Next steps" in `BACKLOG.md`.

## After verification — memory and pipeline (me)
Once Verification passes:
- **Project memory:** save the library/platform facts and harness limits confirmed in this run (from the agents' final reports and retros). Update an existing entry rather than adding a duplicate, fix or delete entries the run proved wrong, and keep the `MEMORY.md` index line in sync. Project-specific knowledge goes there, not in this plan. Skip it when the run taught nothing new.
- **Pipeline suggestions:** merge this run's retro "What to change" items into `pipeline/suggestions.md`: add this slug to the `runs` of a matching line, or add a new `open` line. Apply every `open` suggestion that now has 2 or more runs to this plan (or to the tools in `pipeline/tools/`) and mark it `applied`; mark one `rejected: <reason>` when it conflicts with the plan or isn't worth it. Apply a single-run suggestion right away only when it is a clear win (a prompt fact that was missing, a broken command). `PIPELINE-BACKLOG.md` is tracked: if it changed, commit it alone ("Update pipeline backlog after <slug>") and push it (no tag, no version bump), so the next run's Step 0 starts with a clean tree.
- Append `<time> | <slug> | pipeline | docs | FEATURES/PLAN: <changed or "no changes">, memory: <entries updated or "none">, plan: <suggestions applied or "none">` to `pipeline/log.txt`.

## Loop until BACKLOG.md is done
When Verification and the memory and pipeline step are done, the run is over: I start a new run from Step 0 with the next first item under "Next steps" in `BACKLOG.md`, without waiting for the user. I repeat until "Next steps" is empty, then append `<time> | all | pipeline | end | plan done, <runs> runs, <test count>` to `pipeline/log.txt` and give a final report: one line per run (slug, squashed commit, test count, findings fixed/skipped, tokens and duration), the steps marked not doable with their reasons, and the suggestions applied or rejected in `pipeline/suggestions.md` during the loop.
- Each run still ends with its own "After all agents" report, kept short, and its memory and pipeline update, so a later stop loses nothing.
- **Step not doable:** if a step can't be done or checked on this machine (e.g. it needs Windows, a real device or a user decision the plan doesn't give), whether I see it in Step 0 or agent 1 reports it, I don't stop the loop:
  - Agent 1 stops before committing and reverts its own uncommitted changes (leaving files it didn't touch alone), then says why in its final report.
  - I move the step from "Next steps" to a "Not doable yet" section in `BACKLOG.md` (create it after "Next steps" if missing), with the reason in one line, and commit `BACKLOG.md` alone: "Mark <step> as not doable yet". If Step 0 made a split commit for this run, squash it in (`git reset --soft <base>` and commit again with that message), so there is still one commit. Then push it (no tag, no version bump).
  - Append `<time> | <slug> | pipeline | end | not doable: <reason>` to `pipeline/log.txt` and start the next run.
- The loop stops (and doesn't start the next run) on anything else that stops a run: a red baseline or gate, a failed Verification check, uncommitted changes Step 0 can't explain, or a review finding or UX/UI question that needs a user design choice (UX/UI questions the UX/UI agent decides don't stop it). I report why and which steps are left; the user restarts the loop after deciding.
