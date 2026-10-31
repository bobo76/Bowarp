# Plan: pluggable agents

Goal: adding an agent CLI (after Claude Code and Copilot CLI) should mean one new module per
crate it touches, not edits across ~13 files that branch on `SessionKind::ClaudeCode` /
`CopilotCli`.

## Design

The set of agents is fixed at compile time, so: an **`AgentId` enum + static per-agent data +
one behavior module per agent**, not trait objects. Adding a variant makes the compiler list
every `match` that needs an arm, and those matches live in a few dispatch functions. A
`dyn AgentIntegration` trait would also work, but costs boilerplate and loses exhaustiveness.

Placement follows the crate graph (`workspace-model` ← `term-core`, `agent-status` ← `app`):

- `AgentId` and its data: **`workspace-model`** (lowest crate; already holds `LauncherIcon`,
  `SavedKind`).
- Status rules and hook config generation: **`agent-status`**.
- Hook file setup, icons, UI: **`app`**.

## Steps

Each step is one commit passing the check; no behavior change until the end.

### 0. Renames

`ClaudeHooks` → `AgentHooks` (`app/src/hooks.rs`, `app/src/main.rs`): it serves both agents.

### 1. `AgentId`

New `workspace-model/src/agents.rs`:

```rust
pub enum AgentId { Claude, Copilot }

impl AgentId {
    pub const ALL: &[AgentId];
    pub fn id(self) -> &'static str;       // "claude": launcher/settings/CLI key, never rename
    pub fn label(self) -> &'static str;    // "Claude Code"
    pub fn program(self) -> &'static str;  // "claude"
    pub fn from_id(id: &str) -> Option<Self>;
    pub fn capabilities(self) -> Capabilities;
}

pub struct Capabilities {
    pub status: StatusSource,        // Hooks | ProgressWithWaitHooks
    pub mode_bridge: bool,           // Copilot extension's mode reports
    pub spinner_when_working: bool,
    pub extra_env: &'static [(&'static str, &'static str)],  // ConEmuANSI=ON
    pub inherited_env_markers: &'static [&'static str],      // CLAUDECODE, CLAUDE_PID, ...
}
```

Unit tests: id round-trip, ids unique.

### 2. `SessionKind` = `Shell | Agent(AgentId)`

In `term-core/src/launch.rs`:

- `LaunchSpec::claude_code` / `copilot_cli` → `LaunchSpec::agent(id, cwd)`.
- `term-core/src/session.rs` (`session_env`): use `capabilities().extra_env` instead of the
  `kind == CopilotCli` check.
- `app/src/main.rs`: parse the CLI with `AgentId::from_id`; build the env scrub list from every
  agent's `inherited_env_markers` plus the generic `AI_AGENT`.
- Update the remaining call sites mechanically (`kind != Shell` checks stay).

### 3. Saved tabs and settings (backward compatible)

- `SavedKind`: keep the `"claude-code"` / `"copilot-cli"` strings on disk; add a test loading an
  existing `tabs.json`. Check what an unknown kind does today: ideally that tab is skipped, not
  the whole file rejected, so a downgrade doesn't drop every tab.
- Settings: keep the flat `claude_args` / `copilot_args` fields (users edit the file by hand);
  add `Settings::agent_args(id)` / `set_agent_args(id, value)` as the only `match`. Each new
  agent adds one `<id>_args` field.
- `app/src/launcher.rs` (`configured_spec`, `AGENTS`) and the agent rows in
  `app/src/settings_page.rs` iterate `AgentId::ALL`.

### 4. Per-agent behavior in `agent-status`

```
agent-status/src/agents/mod.rs      // dispatch: hook_config(id, ..), apply_hook(id, state, ..)
agent-status/src/agents/claude.rs   // claude_settings(), HOOK_EVENTS, the apply() rules
agent-status/src/agents/copilot.rs  // copilot_hooks(), COPILOT_HOOK_EVENTS, apply_copilot_hook
```

- `AgentState` keeps the shared primitives (`apply`, `apply_progress`, `exit`); agent modules
  decide which to call.
- The `ipc.rs` wire format (`CopilotMode`, camelCase normalization) stays as is: the shipped JS
  extension depends on it.
- Tests move with their code.

### 5. Per-agent hook setup in `app`

```
app/src/hooks.rs          // AgentHooks: server, start, stale sweep, attach() dispatch
app/src/hooks/claude.rs   // one shared --settings file per instance
app/src/hooks/copilot.rs  // per-session --plugin-dir and its cleanup
```

- `attach` returns `HookAttachment { args_prefix, env, status_by_hooks }` instead of mutating
  the spec in each branch, so it is unit-testable without a session.
- `OwnedEntry` (stale-file sweep) gets one variant per agent's file layout.

### 6. `terminal_view.rs`: capabilities instead of kind checks

- `tab_status`: `Unavailable` when `status == Hooks`.
- `apply_hook_event`: one arm calling `agents::apply_hook(id, ...)`; "adopt the tracker on the
  first wait hook" becomes a `ProgressWithWaitHooks` check.
- `apply_progress`: adopt when `status == ProgressWithWaitHooks`.
- Mode reports: apply when `mode_bridge`.
- `on_bell`: suppressed when `status == Hooks`.

### 7. Icons

- `LauncherIcon::Claude` / `Copilot` → `LauncherIcon::Agent(AgentId)`.
- `app/src/sidebar.rs`: one `agent_icon(id)` match (icons are assets, so they stay in `app`);
  `working_spinner` uses `spinner_when_working`.

### 8. Docs

Update the CLAUDE.md architecture section; add `docs/adding-an-agent.md` with the checklist
below.

## Adding agent #3 afterwards

1. Variant + data/capabilities in `workspace-model/src/agents.rs`.
2. `agent-status/src/agents/<name>.rs`: status rules, hook config if it has hooks.
3. `app/src/hooks/<name>.rs` if it needs hook files.
4. Icon asset + arm in `agent_icon`.
5. `<name>_args` settings field + arm in `agent_args`.
6. Fix the non-exhaustive `match` errors the compiler reports; add tests.

An agent with neither hooks nor progress reporting needs only steps 1, 4 and 5: it falls back
to output-activity status like shells. That needs a third `StatusSource::OutputActivity`,
added only when such an agent arrives.

## Risks and open questions

- Compatibility: `tabs.json`, `settings.json` and the extension wire names stay the same on
  disk; tests pin them.
- Windows: hook file paths move but the logic doesn't change; covered by
  `scripts/test-windows.sh`.
- Size: ~15 files, mostly mechanical; steps 4–6 carry the logic and already have tests.
- Which agent is third? If known, check how it reports status before step 1, so
  `Capabilities` is shaped by three agents rather than two.
