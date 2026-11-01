<!-- typed-agent-switch 873db5f -->
# Review of 873db5f: switch for agent status of claude and copilot typed at the prompt

Scope: the diff and what it relies on (checklist given in the prompt). FM IPL second pass
skipped: ~74 lines of non-test Rust changed (< ~100). Toolchain pass skipped (green on 873db5f,
incl. Windows CI run 41339340340).

Checked, no finding: the flag is read and removed at the top of agentws.bash / .zshenv /
agentws.ps1, before any user file or child process (bash: Git Bash login path too, since it is
removed before `/etc/profile`); the shell variable is unset on every path (bash: after the
if/else at the end; zsh: in `__agentws_install` and in the `else` branch; PowerShell:
`Remove-Variable` after the block, the script is dot-sourced so the variable is global until
then). WSL: `WSLENV` carries the flag; the bash path's script removes it, `WSL_LAUNCH` unsets
it for other login shells and when `getent` finds no shell. cmd gets no flag
(`has_agent_functions`), shells not attached (fish, shells with arguments) get none since
`configure` isn't reached. Nested shells get no token, so no wrappers. Re-sourcing is covered by
the existing `__agentws_loaded` guards. `set -u` / `Set-StrictMode`: the variables are always
assigned (`${..-}`, `$env:X -eq '1'`). `attach` has one production caller
(`workspace_view.rs:527-528`), tests updated. Setting: `FeatureSwitch<true>`, lenient, after
`copilot_args`, read-modify-write through `settings_file`; row title unique on the Terminal page;
page-level Reset covers it like the other switches. One settings read per spawn: no redraw cost.
MSRV: no new std API. No bytes printed by the scripts change.

## Design choices

### 1. Low - a missing flag means off

`crates/app/shell-integration/agentws.bash:118`, `zsh/.zshenv:130`, `agentws.ps1:12`

```bash
if [ "$__agentws_typed_agents" = 1 ] && [ -f "${BASH_SOURCE[0]%/*}/agentws-agents.sh" ]; then
```

Problem: the scripts are rewritten by each app start (`scripts_dir`, `shell_integration.rs:239`)
and several instances can run at once. An instance of the previous version (wrappers always on,
no flag) still running after an upgrade starts its new tabs with the new scripts and no flag, so
its typed agents lose status until it restarts, although the setting's default is on.

Options:
- A (keep, recommended): only `1` turns the wrappers on. Anything that isn't the current app
  (old instance, a hand-run `bash --rcfile`) gets plain `claude`; the window is one app
  restart, and the token check already needs the app.
- B: missing = on, only `0` turns them off (`[ "${__agentws_typed_agents-1}" != 0 ]`, zsh
  `${__agentws_typed_agents-1} != 0`, PowerShell `$env:AGENTWS_TYPED_AGENTS -ne '0'`), matching
  `FeatureSwitch<true>`'s default. Changes agentws.ps1 (Windows CI re-run) and the meaning of an
  empty value.

Source: checklist

**Verdict:** Confirmed. **Worth doing?** No: user picked option A (missing flag = off), as built. **Status:** Skipped.

## Mechanical

### 2. Low - script header comments left half-wrapped

`crates/app/shell-integration/agentws.bash:9`, `crates/app/shell-integration/agentws.ps1:8`

```
# Needs bash 4.4 (PS0). With AGENTWS_BASH_LOGIN set
# AGENTWS_TYPED_AGENTS is 1. Keep it Windows
```

Fix: reflow the sentences to the 100-char width (comment-only, agentws.ps1 included).
Source: checklist. **Verdict:** Confirmed (self). **Status:** Fixed.

### 3. Low - inline settings group in `terminal_groups`

`crates/app/src/settings_page.rs:296-310`

```rust
        agent_arguments_group(&launchers),
        SettingGroup::new().title("Agents at the prompt").item(
            switch_row(
```

Problem: every other Terminal group is a named helper (`command_history_group()`,
`agent_arguments_group()`, `clipboard_writes_row()`); the 15-line inline group breaks the list.
Fix: move it unchanged (same title, text, keywords) into `fn typed_agents_group() ->
SettingGroup` and call it in the list.
Source: checklist. **Verdict:** Confirmed (self). **Status:** Fixed.

## Test gaps (no finding)

- `WSL_LAUNCH`'s non-bash path is checked as a string only (no WSL here); acceptable.
- No end-to-end `attach(.., false)`: `configure` is covered per shell incl. cmd's `None`.

## Check after fixes

fmt, build, `cargo test --no-fail-fast` 640 passed / 0 failed (memory's count command),
clippy `-D warnings` clean, comment width clean. agentws.ps1 changed (comment only).
