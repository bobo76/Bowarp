# Pipeline: next PLAN.md step → Rust review → verify & fix

## Context
A reusable pipeline: each run takes the first item under "Next steps" in `PLAN.md` and runs three agents one after another: implement and commit, review with the Rust checklist and the `review-fm-ipl` skill into `REVIEW.md`, then verify the findings, fix what's worth it and commit. Each agent logs when it starts and ends, and writes a retrospective on the pipeline itself when it's done. When a run is done, its commits (up to ~4: a plan decision, the step, the review fixes, the docs update) are squashed into one and pushed. Runs follow one another until "Next steps" is empty (see "Loop until PLAN.md is done").

## Pipeline files (git-ignored)
Everything the pipeline produces about itself lives in `pipeline/` at the repo root, which is listed in `.gitignore`:
- `pipeline/log.txt`: one line per event, appended and never rewritten, across all runs:
  `<ISO time> | <slug> | <agent> | begin|end | <task in one line>`
  On `end`, the task part also says the outcome, e.g. `committed 8b77458, 109 tests` or `stopped: test display unavailable`.
- `pipeline/retros/<slug>/`: the 3 retros of a run.
- `pipeline/suggestions.md`: the retros' "What to change" items across runs, one line each: `<suggestion> | runs: <slug>, <slug> | open|applied|rejected: <reason>` (see "After verification").

**The check** (used by the gate, the squash and Verification): `cargo fmt --check && cargo build && cargo test && cargo clippy --all-targets -- -D warnings`. Where `cargo` lives (it may not be on PATH) is in the toolchain memory entry.

Get the time with `date -Iseconds`. Append to the log with `>>` and never edit earlier lines.

Event types: `begin`, `end`, `usage`, `docs` (see "After verification"), `split` (the step was split; see Step 0), `restart` (an agent run again after an interruption; see Step 0), `rebase` and `ci` (see "Squash and push").

## Models
- **Main session (me):** the latest Opus (currently Opus 5.5, `claude-opus-5-5`). If the session runs on another model, I stop and ask the user to switch (`/model opus`) before Step 0.
- **Agents 1 and 2:** the latest Opus. Launch them with the Agent tool's `model` set to `opus`; never leave it unset, so a configured default subagent model can't downgrade them.
- **Agent 3 (verify and fix):** Sonnet, `model: "sonnet"`. It is the only agent that runs on Sonnet.

## UI test tools (kept, git-ignored)
`pipeline/tools/` holds the harness for driving the app in a headless display. Agents use it as is and do not rebuild it; if a tool is missing something a step needs, extend the script rather than writing a one-off. What the scripts are, how to call them and what they can't drive is in project memory, not here.

## Project knowledge (project memory)
Everything specific to the project (toolchain constraints, test harness usage and limits, confirmed library behaviour) lives in project memory, so this plan stays generic. The one exception is agent 2's review checklist, which names the project's stack on purpose: it replaces the generic `review-rust` skill. Each prompt lists the absolute paths of the entries the agent must read first (see "Every agent prompt"). Memory lives in `~/.claude/projects/-home-bobo-dev-agent-workspace/memory/`. Agents only read it; I am the only one who updates it (see "After verification").

## Retro focus
The retros are about **the pipeline, not the step**: how to make the next run faster and use fewer tokens. Each retro answers:
- **Where time and tokens went:** the steps that cost the most, e.g. reading big files in full, repeated builds, test harness setup, exploring code that turned out not to matter.
- **Waste:** work that was redone, context the prompt should have given but didn't, files read but not needed, commands that failed and had to be retried.
- **What to change:** concrete edits to this plan or the agent prompts (context to pass in, files to skip, scripts or helpers to keep, checks to merge or drop), each with the expected saving.
- **Step notes:** at most a few lines about the step itself, and only when they affect the pipeline. Product choices and what's untested go in `PLAN.md` and the final report, not here.

## Step 0 — me, before any agent
In this order:
1. **Resume check, first:** look in `pipeline/log.txt` for a `pipeline | begin` line with no matching `pipeline | end` for the same slug. If one exists, the last run was interrupted: take the slug and base from that line, not from "Next steps", because agent 1's commit may already have removed the step from it. Then find the last agent with a `begin` and no `end`, and look at git log, the working tree and `pipeline/retros/<slug>/` to see how far it got:
   - The agent's commit exists (for agent 2: `REVIEW.md` is newer than agent 2's `begin` time and its first line names this slug): run its gate, write the missing `end` line (`end | recovered: <outcome>`), and continue with the next agent.
   - Uncommitted changes from the interrupted agent: I stop and ask whether to keep them (relaunch the agent telling it to finish and commit them) or discard them and rerun.
   - Nothing usable: append `<time> | <slug> | pipeline | restart | <what was interrupted>` and rerun that agent.
   In every case, skip the rest of Step 0; the gate covers the baseline.
2. **Dirty tree:** if the working tree has uncommitted changes, I stop and ask whether to commit them (and with which message) or abort. This comes before any commit of the run, so a split never sweeps the user's `PLAN.md` edits into it.
3. Read the first item under "Next steps" in `PLAN.md`; that is **the step** for this run. Derive a short slug from it (e.g. `m2-part-e`) for the retro folder. If "Next steps" is empty, I stop and report. If the step plainly can't be done on this machine, I mark it not doable and move on (see "Loop until PLAN.md is done") without launching any agent.
4. Run `cargo build` and `cargo test`, so we know the baseline is green. If it's red, I stop and report back.
5. **Record the base:** `git rev-parse HEAD`, then append `<time> | <slug> | pipeline | begin | <step text> (base <sha>)` to `pipeline/log.txt`, before any commit of this run, so a resumed run can find it.
6. **Step size check:** if the step bundles several features that could each ship alone (e.g. a protocol, a store and two UI pieces), or would likely add more than ~1 500 lines, split it in `PLAN.md` into ordered sub-steps that each stand alone and can be tested, and commit that as the run's plan decision commit. Then take the first sub-step as the step, and append `<time> | <slug> | pipeline | split | <first sub-step>` (the slug stays the same for the run).

## Every agent prompt
- First command: append `begin` with the agent's task to `pipeline/log.txt`. Last command, after the commit and the retro: append `end` with the outcome.
- Stage files by path; never `git add -A` or `git add .`. Leave uncommitted files that the agent didn't change alone (they may be the user's notes).
- Never stage `pipeline/`, `REVIEW.md`, and don't edit or stage `PIPELINE-PLAN.md` (it is tracked, but only I change and commit it; see "After verification"). Don't edit `CHANGELOG.md`; I write it when the version is bumped.
- "Before anything else, read these memory files:" followed by the absolute paths of the entries marked for every agent (toolchain facts) and of those relevant to the step (library behaviour, test harness usage and limits). Paths only, never the text of the entries. Tell the agent not to edit them.
- Ask for a final report that lists the library/platform facts the code relies on, each with the source `file:line` that confirms it, so the next agent checks those lines instead of searching again. For agent 1 this also includes the `file:line` of each state change the feature relies on (where a field is set, cleared and read, which event triggers a redraw), so the reviewer can follow the state flow without a grep round.
- For agent 2: the path of agent 1's retro and agent 1's final report (what was verified and how, what was not driven, the choices made). Read those before the code instead of working them out again.
- For agent 3: agent 2's final report and any user decisions; point to `REVIEW.md` for the findings instead of pasting them, and use REVIEW.md's finding numbers. No retros needed.
- Tell agents to tee `cargo test` output to a scratch file and count from it, not run the tests twice, and to take the app's real shortcuts from project memory rather than guessing them.

## Agent 1 — implement the step (general-purpose, `model: "opus"`, I wait for it)
Launch this agent with the Agent tool's `model` set to `opus`.
Prompt contents:
- The step text from `PLAN.md`, verbatim.
- Read `PLAN.md` (the step, plus any reference screenshot it links to) and `FEATURES.md` (including "Optional Behavior" when the step adds a switch), then the code the step touches.
- Where the step is ambiguous (key bindings, edge cases, exact behaviour), pick a sensible option consistent with the existing code and list the choice in its final report.
- In the final report, list behaviour that changed for existing paths (e.g. tab close no longer keeps viewer edits) separately from new-feature choices: that is where regressions hide.
- If the step can't be done or checked on this machine, stop before committing, revert your own changes and give the reason (see "Step not doable" under the loop section).
- Rules:
  - Follow the existing code style.
  - Put pure logic in a non-UI crate with unit tests.
- Verify with `cargo fmt`, `cargo clippy --all-targets`, `cargo build` and `cargo test`. If the step has UI, drive the app with the scripts in `pipeline/tools/` (see "UI test tools"); don't set up the display by hand. Say up front which parts the harness can't drive.
- Update `PLAN.md`: remove the step from "Next steps" and add the checks the harness couldn't drive under "Fixed, not tested yet" (the rest of the cleanup happens after Verification). Add any new known limitation to `docs/known-limitations.md`. No test counts or verification records in `PLAN.md`; the test count goes in the `end` log line.
- Commit the code, `PLAN.md` and `docs/known-limitations.md` (if changed) with an imperative summary of the step as the message.
- Write `pipeline/retros/<slug>/1-implement.md` (see Retro focus).

## Agent 2 — Rust + FM IPL review (general-purpose, `model: "opus"`, after agent 1 finishes)
Launch this agent with the Agent tool's `model` set to `opus`.
- Review agent 1's commit with a short project checklist in the prompt instead of loading the full `review-rust` skill (most of its sections don't apply to a GPUI diff, and it costs ~6k tokens). The checklist: ownership/clones in render paths, `Task` lifetimes (dropped = cancelled), redraw cost (what notifies, how often, while idle), state set/cleared on every path (exit, cancel, re-entry), bytes typed into the PTY, error handling (`expect`/`unwrap` on I/O), APIs newer than the MSRV (see the toolchain memory entry), and test gaps. Load the skill only for `unsafe`, FFI, async runtime or concurrency changes. Review (`git show <sha>`): the changed code, the callers and callees it changes, and the state flow it relies on. Don't review older code outside that, and don't read whole files when the diff and a targeted read will do.
- **Second pass, FM IPL standards:** also load the `review-fm-ipl` skill (Skill tool) and review the same diff against its Clean Code (CC-x) and Object Calisthenics (OC-x) rules. The skill is written for Java, so apply the rules to the Rust equivalents (function and module size, single responsibility, naming, nesting, primitive obsession, etc.) and skip rules with no Rust meaning. Every finding from this pass cites its rule code (e.g. `CC-3`, `OC-5`) and goes into the same `REVIEW.md`, numbered in sequence with the checklist findings, with a `Source:` line (`checklist` or `review-fm-ipl`). Same scope as the checklist pass: only code the diff adds or changes.
- Skip any toolchain pass (also when the skill is loaded): the gate already ran fmt, clippy and test on this commit and they're green. Findings come from reading the code.
- **Overwrite** `REVIEW.md` with the findings. Its first line is `<!-- <slug> <reviewed sha> -->`, so a resumed run can tell it belongs to this run. Each finding gets a number, a severity, `file:line`, the problem, a suggested fix, and **the relevant lines quoted** (a few lines, enough for agent 3 to check the finding without searching for the code). When the fix is under ~10 lines, give it as a near-ready diff.
- List findings that need a design choice first in `REVIEW.md`. Flag them with the options, and name the existing API or code each option would use (or say there is none), so agent 3 doesn't explore for it. If there are any, I stop the loop before launching agent 3 (append `<time> | <slug> | pipeline | end | stopped: design choice needed on #<n>`) and ask the user to pick. When the user restarts, the resume check continues with agent 3 and I pass the decision in its prompt.
- Code changes: none. No commit. Exception: when every finding is Low, mechanical and has a ready diff (comment wrap, typo, rename) and none needs a design choice, agent 2 applies them, runs fmt/clippy/build/test, commits "Address Rust review findings", marks each finding **Verdict:** Confirmed (self) and **Status:** Fixed in `REVIEW.md`, and agent 3 is skipped (saves a ~40k-token agent).
- Write `pipeline/retros/<slug>/2-review.md` (see Retro focus).

## Agent 3 — verify and fix (general-purpose, `model: "sonnet"`, after agent 2 finishes)
Launch this agent with the Agent tool's `model` set to `sonnet`.
- For each finding in `REVIEW.md`, check the quoted lines and confirm or refute it. Read more code only when the quote isn't enough to decide (e.g. whether a state is cleared elsewhere). Next to each problem, add a **Verdict** (Confirmed / Refuted / Partly), a **Worth doing?** (Yes / No + one-line reason) and later a **Status** (Fixed / Skipped).
- Fix every finding marked "Yes", all in one batch, then run fmt, clippy, build and test. Iterate until it's green. If a fix is stuck after 2 attempts, mark it Skipped with the reason.
- Commit the fixes (`REVIEW.md` is git-ignored and stays local): "Address Rust review findings".
- Write `pipeline/retros/<slug>/3-fix.md` (see Retro focus).

## Gate between agents
Before launching the next agent, I check that:
- the expected commit exists (agent 1: the step's commit; agent 3: review fixes commit), or for agent 2 that `REVIEW.md` was rewritten and the working tree has no code changes (or, when agent 2 applied the fixes itself, its review fixes commit exists and agent 3 is skipped);
- the check (see "Pipeline files") is green; skip it when HEAD hasn't moved since the last gate (agent 2 with no fixes), because that commit was already checked;
- the agent's retro exists in `pipeline/retros/<slug>/`;
- `pipeline/log.txt` has the agent's `begin` and `end` lines.

Then I append `<time> | <slug> | <agent> | usage | <tokens> tokens, <tool uses> tool uses, <duration>` to `pipeline/log.txt`, from the agent's completion notice, so the retros of later runs can compare costs.

If any check fails, including agent 1 being unable to run a needed UI check, I stop the pipeline, append `<time> | <slug> | pipeline | end | stopped: <reason>`, and report what failed instead of launching the next agent.

## After all agents
- **Docs:** bring the docs in line with what shipped, from the agents' final reports, so they go out in the same squashed commit:
  - **`FEATURES.md`:** if the step added or changed user-visible behaviour that no line describes (or describes differently), add or reword the line in the matching section. It stays a spec of the product, not a changelog: no step names, versions or file paths. Nothing to change for internal work (refactors, fixes of behaviour already described).
  - **Clean `PLAN.md` of what's done:** finished work lives in git history, not here. Remove what this run completed: backlog items the step covered (trim an item to its unfinished part when only part of it is done) and "Fixed, not tested yet" entries the step resolved. Also remove entries in `docs/known-limitations.md` the step lifted. Don't touch items the run didn't cover.
  - If any of these files changed, commit them alone: "Update docs after <slug>".
- **Squash and push:** the run's commits are `<base>..HEAD` (usually 2-4: a `PLAN.md` decision commit if any, the step, the review fixes, the docs update).
  - Check first that every commit in `git log <base>..HEAD` belongs to this run. If another one landed mid-run (e.g. the user's own commit), I stop and ask instead of squashing it in.
  - **Sync with the remote before the bump:** `git fetch origin`. If the remote moved (the user commits mid-run), `git rebase origin/<branch>` the run's commits, resolve conflicts keeping both sides (the user's PLAN.md edits win, minus the step just done), set `<base>` to `origin/<branch>` (log `<time> | <slug> | pipeline | rebase | new base <sha>`) and re-run the check. Doing this before the bump means a version bump or CHANGELOG edit by the user is picked up instead of conflicting with ours.
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
- `pipeline/retros/<slug>/` holds 3 files (2 when agent 2 applied the fixes), `pipeline/` doesn't show up in `git status` or `git log`, and the squashed commit doesn't touch `PIPELINE-PLAN.md`.
- The step is gone from "Next steps" in `PLAN.md`.

## After verification — memory and pipeline (me)
Once Verification passes:
- **Project memory:** save the library/platform facts and harness limits confirmed in this run (from the agents' final reports and retros). Update an existing entry rather than adding a duplicate, fix or delete entries the run proved wrong, and keep the `MEMORY.md` index line in sync. Project-specific knowledge goes there, not in this plan. Skip it when the run taught nothing new.
- **Pipeline suggestions:** merge this run's retro "What to change" items into `pipeline/suggestions.md`: add this slug to the `runs` of a matching line, or add a new `open` line. Apply every `open` suggestion that now has 2 or more runs to this plan (or to the tools in `pipeline/tools/`) and mark it `applied`; mark one `rejected: <reason>` when it conflicts with the plan or isn't worth it. Apply a single-run suggestion right away only when it is a clear win (a prompt fact that was missing, a broken command). `PIPELINE-PLAN.md` is tracked: if it changed, commit it alone ("Update pipeline plan after <slug>") and push it (no tag, no version bump), so the next run's Step 0 starts with a clean tree.
- Append `<time> | <slug> | pipeline | docs | FEATURES/PLAN: <changed or "no changes">, memory: <entries updated or "none">, plan: <suggestions applied or "none">` to `pipeline/log.txt`.

## Loop until PLAN.md is done
When Verification and the memory and pipeline step are done, the run is over: I start a new run from Step 0 with the next first item under "Next steps" in `PLAN.md`, without waiting for the user. I repeat until "Next steps" is empty, then append `<time> | all | pipeline | end | plan done, <runs> runs, <test count>` to `pipeline/log.txt` and give a final report: one line per run (slug, squashed commit, test count, findings fixed/skipped, tokens and duration), the steps marked not doable with their reasons, and the suggestions applied or rejected in `pipeline/suggestions.md` during the loop.
- Each run still ends with its own "After all agents" report, kept short, and its memory and pipeline update, so a later stop loses nothing.
- **Step not doable:** if a step can't be done or checked on this machine (e.g. it needs Windows, a real device or a user decision the plan doesn't give), whether I see it in Step 0 or agent 1 reports it, I don't stop the loop:
  - Agent 1 stops before committing and reverts its own uncommitted changes (leaving files it didn't touch alone), then says why in its final report.
  - I move the step from "Next steps" to a "Not doable yet" section in `PLAN.md` (create it after "Next steps" if missing), with the reason in one line, and commit `PLAN.md` alone: "Mark <step> as not doable yet". If Step 0 made a split commit for this run, squash it in (`git reset --soft <base>` and commit again with that message), so there is still one commit. Then push it (no tag, no version bump).
  - Append `<time> | <slug> | pipeline | end | not doable: <reason>` to `pipeline/log.txt` and start the next run.
- The loop stops (and doesn't start the next run) on anything else that stops a run: a red baseline or gate, a failed Verification check, uncommitted changes Step 0 can't explain, or a review finding that needs a user design choice. I report why and which steps are left; the user restarts the loop after deciding.
