<!-- typed-agent-status 5f711e5 -->
# Review of 5f711e5 "Show agent status in place for an agent marked in a shell tab"

Eight findings, all Low, all fixed (4, 5, 7, 8 in cf74e08 "Address Rust review findings"; 1-3
and 6 by agent 3; 1 and 2 per user decisions). No deviation from docs/ux-decisions.md "In-place
agent in a shell tab" / "When the in-place run ends" found.

Checked and fine (no finding): `$?` capture order (bash `local status=$?` first in the first
PROMPT_COMMAND entry, before `__agentws_update_cwd`; zsh 5.9 gives every precmd hook the command's
status even after an earlier hook ran `false`, checked with a real `zsh -i` and two
`precmd_functions`; `set -u` safe, both are locals), N honoured only while `Phase::Running` and
only with the session token, unknown agents refused (`from_cli_selector`), hook reports dropped
outside a run (`status_kind().agent()`), late progress/title after D ignored for status, plugin
dir removed on every close path (`remove_session` at workspace_view.rs:541/645/686/1032), no new
idle redraws (start/end emit one TabChanged each), no `unwrap`/`expect` on I/O, no post-1.95 APIs,
no unsafe/concurrency changes.
