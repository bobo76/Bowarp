# Review: staged `SessionKind` / `AgentId` refactor (branch `backlog`)

Saved: 2026-10-31.

Build passes, `cargo fmt --check` fails, clippy fails (2 errors), tests pass.

## Must fix before committing (these break "the check")

1. **Clippy errors** (`-D warnings`):
   - `crates/app/src/hooks.rs:128`: `redundant_guards`. `Some(agent) if agent == AgentId::Copilot`
     should be `Some(AgentId::Copilot)`.
   - `crates/app/src/hooks.rs:145`: `needless_return`. `return false;` should be `false`.
2. **fmt diffs:**
   - `crates/app/src/sidebar.rs:177,180`: the `return kind_icon(...)` lines are missing their `;`.
   - `crates/app/src/terminal_view/tests.rs:60`: the match arm needs rewrapping.
   - `crates/workspace-model/src/lib.rs:19,55`: `mod agents;` and its `pub use` come before
     `about`. rustfmt sorts these, so run `cargo fmt --all` to fix them all at once.

## Design issues

3. **[High] Each agent now has two kinds that don't compare equal.** `SessionKind` has both
   `ClaudeCode` and `Agent(AgentId::Claude)` (likewise for Copilot), and the derived `PartialEq`
   treats them as different.
   - Production code never creates `Agent(_)`: `LaunchSpec::agent`
     (`crates/term-core/src/launch.rs:73`) maps straight back to `ClaudeCode`/`CopilotCli`. So the
     new variant only adds extra match arms.
   - Some code still compares with `==`, for example the asserts at
     `crates/app/src/main.rs:346,349` and `crates/app/src/launcher/tests/unix.rs`. Once `Agent(_)`
     is actually created, those comparisons stop matching without any error.
   - `tabs_file.rs:188` folds `Agent(_)` back into the legacy saved kinds, so the two forms can't
     be told apart after a restore.
   - `LauncherIcon::Agent` (`crates/workspace-model/src/launcher_icon.rs:22`) has the same
     problem: it is never created.
   - **Fix:** pick one form. Either replace `ClaudeCode`/`CopilotCli` with `Agent(AgentId)`
     everywhere, or drop `Agent(_)` for now and keep just `agent_id()` and the `is_*` helpers.
4. **[Medium] New API with no callers.** These are `pub`, so clippy doesn't flag them:
   - `AgentId::capabilities`, `Capabilities` and `StatusSource`
     (`crates/workspace-model/src/agents.rs:43-91`)
   - `Settings::set_agent_args` (only used by its test)
   - `From<AgentId> for SessionKind`

   Project policy is to add an API together with its first caller. Also, `extra_env:
   CLAUDECODE=1` would, once wired up, set in the child the variable Claude Code uses to detect
   it is running nested inside another session. Check that this is intended before using it.
5. **[Medium] Agent ids and labels are defined in two places.** `launcher::AGENTS`
   (`crates/app/src/launcher.rs:93`) still hard-codes `"claude"`/`"Claude Code"`/kind/icon, which
   `AgentId` now duplicates. Build `AGENTS` from `AgentId::ALL` so they can't drift. Also,
   `AgentId::id()` and `program()` currently return the same strings.
6. **[Low] The new CLI fallback is mostly unreachable.** In `crates/app/src/main.rs:80`,
   `"claude"` and `"copilot"` are matched first, so `from_id` only adds the labels
   (`agentws "Claude Code"`) and whitespace trimming. That seems unintended. Also, `folder` is now
   computed before the kind is checked.

## Conventions

7. **[Low] Fully qualified paths inline**, against the global CLAUDE.md:
   - `workspace_model::AgentId::…` in `sidebar.rs`, `tabs_file.rs`, the `session.rs` tests and
     the `terminal_view/tests.rs` tests
   - `crate::AgentId` in `settings.rs:269-280`

   Import `AgentId` instead.
8. **[Low]** None of the `pub` items in `agents.rs`, or the new `SessionKind` methods, have `///`
   docs.

## Tests

9. **[Medium] Gaps in coverage:**
   - no unit test for `SessionKind::agent_id` / `is_claude` / `is_copilot` / `is_shell`
   - none for `LaunchSpec::agent`
   - none for the new `parse_launch_args` fallback
   - none for the new `attach` match arms in `hooks.rs`

   The existing tests that loop over both forms do pass.
