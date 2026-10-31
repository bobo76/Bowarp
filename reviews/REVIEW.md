<!-- history-per-shell bad946b -->
# Review: bad946b "Keep command history per shell"

14 findings, all Low (no Critical/High/Medium). 1 needs a design choice (not UX), 1 is a UX
nit (optional). Not applied: finding 1 needs a decision.

Checked and found sound (no finding): per-shell `counts` on `add` / `remove` (recount) /
`clear` / `from_jsonl` (zero counts retained away; cap ≥ 1 so the cap drop never empties a
shell); dedup on (shell, command) with legacy `None` kept apart and merged only by "All"'s
newest-per-command view; `needs_compaction` (`lines > max(len, cap) + cap/2`) cannot loop since
a compacted file has `len` lines; file `remove_command` uses the same `covers` rule as memory
and stays read-modify-write under `lock_history`; Alt capture: no chips -> no key taken, unused
letters fall through, Ctrl+Alt excluded (Windows AltGr), and Linux AltGr is Mod5/ISO_Level3,
not `MOD_NAME_ALT`, so `modifiers.alt` is false for it
(`gpui-pre-linux-0.3.9/src/linux/platform.rs:1593`); scoped forget -> `last_command` of
covered tabs only; no new idle redraw (no cursor blink; chips are built only while the popup is
open); no `unwrap`/`expect` on I/O; no post-1.95 std API; no new bytes typed into the PTY.

## Needs a design choice

### 1. Low — crates/app/src/workspace_view.rs:426-432 — Source: checklist
**Verdict:** Confirmed. **Worth doing?** No (user decision A: keep as documented). **Status:** Skipped.
With "Restore previous tabs" off, a Delete under a shell chip scrubs the command from every
saved tab in `tabs.json`, whatever that tab's shell (already listed in
docs/known-limitations.md:58-67). Live tabs follow the scope; the saved ones don't.

```rust
        if seen.1 != forgets_seen
            && let Some((command, _)) = cx.global::<history::History>().forgotten()
        {
            // The saved tabs hold no shell key: the command goes from every one of them.
            self.tabs_writer
                .scrub_last_commands(Some(command.to_owned()));
```

Options:
- A — Keep (documented). Uses: none. Cheapest; the case needs restore off + a saved
  `last_command` equal to the deleted command in another shell's tab.
- B — Derive each saved tab's key from its saved spec. Uses: `launcher::shell_key(&launchers,
  &spec)` (launcher.rs:714), `TabsWriter::scrub_last_commands` / `Job::Scrub`
  (tabs_file.rs:128), `TabsState::scrub_last_commands` / `scrub_file`
  (workspace-model/src/tabs_state.rs:85,109). The job must carry the scope plus the launchers
  (`Rc<[Launcher]>` is not `Send`: clone into a `Vec<Launcher>`), and `scrub_*` take a
  `Fn(&LaunchSpec) -> bool` predicate. ~40 lines, one more test.
- C — Save the shell key per tab in `tabs.json`. Uses: `TabsState` serde (new optional field),
  `TerminalView.shell_key`. Exact, but a file-format change for a corner case.
Recommendation: A.

### 2. Low, UX (optional) — crates/workspace-model/src/history_shells.rs:29-31,41 — Source: checklist
**Verdict:** Confirmed. **Worth doing?** Yes: chip reads like the others. **Status:** Fixed.
The fallback key `shell` (no program and no `$SHELL`, or a program path ending in a
separator) gets the chip label "shell" (lowercase), since `shell_name` falls back to the key.
The UX decision says "else the key itself", so this is compliant; a capitalized label reads
like the other chips.

```rust
    let Some(program) = program else {
        return "shell".into();
    };
```

Fix (if wanted):
```diff
     ("wsl", "WSL"),
+    ("shell", "Shell"),
 ];
```

## Checklist

### 3. Low — crates/workspace-model/src/history.rs:253-279 — Source: checklist
**Verdict:** Confirmed. **Worth doing?** Yes: stale comment, cheap pre-size. **Status:** Fixed.
`matching` runs on every render of the open popup (and in `refresh_history` and
`handle_popup_key`). Under "All" with ≥ 2 shells it now builds a `HashSet` over every match
(up to 2 000 per shell plus legacy), so the comment "no allocation per entry" is no longer
true. Cost only while the popup is open and only on events (no blink), so acceptable; fix the
comment, and pre-size the set.

```rust
        // Called on every frame while the popup is open: no allocation per entry for the usual
        // ASCII query.
...
        let mut seen = HashSet::new();
```

Fix:
```diff
-        // Called on every frame while the popup is open: no allocation per entry for the usual
-        // ASCII query.
+        // Called on every frame while the popup is open: no allocation per entry for the usual
+        // ASCII query, except "All"'s once-per-command set when several shells have commands.
...
-        let mut seen = HashSet::new();
+        let mut seen = HashSet::with_capacity(self.entries.len());
```

### 4. Low — docs/known-limitations.md:58 — Source: checklist
**Verdict:** Confirmed. **Worth doing?** Yes: real downgrade data loss, one line. **Status:** Fixed.
Older versions ignore the unknown `shell` field (no `deny_unknown_fields`) and compact the file
at start once it has more than `cap + cap/2` = 3 000 lines; a per-shell history legitimately
exceeds that (two busy shells = 4 000 lines), so starting an older build (downgrade, or an old
instance started while a new one runs) rewrites the file without shells and caps it to 2 000
in total. Not drivable (agent 1); document it.

Fix: append to the "Command history per shell" item:
```diff
+  Starting an older version rewrites a long history file without shells (all of it then
+  shows only under "All") and keeps only its newest 2 000 commands.
```

### 5. Low — crates/workspace-model/src/history/tests.rs:368-412, 451 — Source: checklist
**Verdict:** Confirmed. **Worth doing?** Yes: guards the incremental counts. **Status:** Fixed.
Test gap: no test checks that the incrementally maintained `counts` equal those of a reload
after mixed adds (with the cap drop and dedup) and removes. `CommandHistory: PartialEq`
compares `counts`, so one assertion covers it.

Fix (end of `commands_are_kept_once_per_shell_and_capped_per_shell`, before the file part,
and after the first `remove` in `removing_in_a_shell_keeps_the_others`):
```diff
+    let (reloaded, _) = CommandHistory::from_jsonl(&history.to_jsonl(), 2);
+    assert_eq!(reloaded, history);
```
(cap 10 in the second test.)

## FM IPL standards

### 6. Low — crates/app/src/history_popup.rs:206-272 — Source: review-fm-ipl (CC-2, CC-3)
**Verdict:** Confirmed. **Worth doing?** No: ~65 lines but cohesive, the split costs an extra chip build per key. **Status:** Skipped.
`handle_popup_key` grew to ~65 lines: it now switches chips and then classifies and applies
popup keys.

```rust
        let chips = self.history_chips(cx);
        let chip = self.history_popup.as_ref().and_then(|popup| {
            switch_chip(
                &chips,
                &popup.scope,
                &keystroke.key,
                held_modifiers(keystroke),
            )
        });
        if let Some(chip) = chip {
            self.select_history_chip(chips[chip].scope.clone(), cx);
            return true;
        }
```

Fix: extract the block.
```rust
    /// Alt + a chip's letter: switch to that chip. Returns whether the key was taken.
    fn switch_history_chip(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let chips = self.history_chips(cx);
        let Some(popup) = &self.history_popup else {
            return false;
        };
        let held = held_modifiers(keystroke);
        let Some(index) = switch_chip(&chips, &popup.scope, &keystroke.key, held) else {
            return false;
        };
        self.select_history_chip(chips[index].scope.clone(), cx);
        true
    }
```
and in `handle_popup_key`: `if self.switch_history_chip(keystroke, cx) { return true; }`
then `let scope = self.history_scope(&self.history_chips(cx));` (one extra chip build per
key; or return the chips from the helper).

### 7. Low — crates/workspace-model/src/history_shells.rs:103-150 — Source: review-fm-ipl (CC-2, CC-3)
**Verdict:** Partly. **Worth doing?** No: one pass, 47 lines; a split adds two helpers for no clearer flow. **Status:** Skipped.
`history_chips` (~47 lines) gathers the keys, decides whether chips show, orders shells and
builds the chips.

```rust
    keys.sort_unstable();
    keys.dedup();
    if !shellless && keys == [own] {
        return Vec::new();
    }
    let mut shells: Vec<(Option<usize>, String, LauncherIcon, &str)> = keys
```

Fix: split into `chip_keys(history, own, selected) -> Option<Vec<&str>>` (`None` = chips
hidden) and `shell_chips(keys, known) -> impl Iterator<Item = ShellChip>` (lookup + sort +
map); `history_chips` chains `All` before them.

### 8. Low — crates/workspace-model/src/history.rs:253-279 — Source: review-fm-ipl (CC-2, CC-3)
**Verdict:** Confirmed. **Worth doing?** Yes: small, behaviour-preserving. **Status:** Fixed.
`matching` filters by scope and text and also dedups for "All".

```rust
        if *scope != HistoryScope::All || self.counts.len() < 2 {
            return found.collect();
        }
        let mut seen = HashSet::new();
        let mut newest_first: Vec<&HistoryEntry> = found
```

Fix: move the tail into
`fn newest_use_per_command<'a>(found: impl DoubleEndedIterator<Item = &'a HistoryEntry>) ->
Vec<&'a HistoryEntry>` and end `matching` with `newest_use_per_command(found)` after the
guard.

### 9. Low — crates/app/src/history_popup.rs:610-615, 768 — Source: review-fm-ipl (CC-9)
**Verdict:** Partly. **Worth doing?** No: .when(flag) is the local GPUI idiom. **Status:** Skipped.
Positional `bool` flags: `history_chip(index, chip, selected: bool, cx)` and
`history_footer(scope, switches: bool, cx)`.

```rust
                let selected = chip.scope == scope;
                self.history_chip(index, chip, selected, cx)
...
fn history_footer(scope: &HistoryScope, switches: bool, cx: &App) -> impl IntoElement {
```

Fix: pass the selected scope and derive the flag inside (`history_chip(index, chip,
&scope, cx)`, `let selected = chip.scope == *selected_scope;`); for the footer pass the chips
(`history_footer(&scope, &chips, cx)` with `.when(!chips.is_empty(), ...)`, building the chip
elements after the footer's borrow) — or keep the flag (optional: GPUI `.when(flag, ..)` is the
local idiom).

### 10. Low — crates/app/src/history_popup.rs:487-507 — Source: review-fm-ipl (OC-2, CC-3)
**Verdict:** Confirmed. **Worth doing?** Yes: guard clause. **Status:** Fixed.
`empty_history_text` returns from inside a `match` arm that initializes `label`, then ends on
an if/else-shaped `match`.

```rust
        let label = match scope {
            HistoryScope::All => return "No commands yet".into(),
            HistoryScope::Shell(key) => chips.iter().find(|chip| chip.scope == *scope).map_or_else(
```

Fix:
```diff
-        let label = match scope {
-            HistoryScope::All => return "No commands yet".into(),
-            HistoryScope::Shell(key) => chips.iter().find(|chip| chip.scope == *scope).map_or_else(
-                || shell_name(key, &launcher::menu_shells(cx)).0,
-                |chip| chip.label.clone(),
-            ),
-        };
+        let HistoryScope::Shell(key) = scope else {
+            return "No commands yet".into();
+        };
+        let label = chips.iter().find(|chip| chip.scope == *scope).map_or_else(
+            || shell_name(key, &launcher::menu_shells(cx)).0,
+            |chip| chip.label.clone(),
+        );
```

### 11. Low — crates/workspace-model/src/history.rs:179-180, 164, 442-443 — Source: review-fm-ipl (CC-12)
**Verdict:** Confirmed. **Worth doing?** Yes: one positive rule for memory and file. **Status:** Fixed.
Negative compound conditions in the `retain` predicates; the positive rule already exists in
`holds_command`.

```rust
        self.entries
            .retain(|entry| entry.command != command || !scope.covers(entry.shell.as_deref()));
...
    serde_json::from_slice::<HistoryEntry>(line)
        .is_ok_and(|entry| entry.command == command && scope.covers(entry.shell.as_deref()))
```

Fix: one positive predicate shared by memory and file:
```rust
impl HistoryEntry {
    /// Whether this is `command` run in a shell `scope` covers.
    fn is(&self, command: &str, scope: &HistoryScope) -> bool {
        self.command == command && scope.covers(self.shell.as_deref())
    }
}
```
then `.retain(|entry| !entry.is(command, scope))` and
`.is_ok_and(|entry| entry.is(command, scope))`.

### 12. Low (optional) — crates/app/src/history_popup.rs:622-632 — Source: review-fm-ipl (OC-2)
**Verdict:** Confirmed. **Worth doing?** No: optional, the suggested form is not clearer. **Status:** Skipped.
`min_width` sums two inline `if/else` expressions.

```rust
        let min_width = CHIP_MIN_WIDTH
            + if chip.icon.is_some() {
                CHIP_ICON_WIDTH
            } else {
                0.
            }
```

Fix:
```diff
-        let min_width = CHIP_MIN_WIDTH
-            + if chip.icon.is_some() { CHIP_ICON_WIDTH } else { 0. }
-            + if rest.is_empty() { 0. } else { CHIP_NAME_MIN_WIDTH };
+        let icon_width = chip.icon.map_or(0., |_| CHIP_ICON_WIDTH);
+        let name_width = (!rest.is_empty()).then_some(CHIP_NAME_MIN_WIDTH).unwrap_or(0.);
+        let min_width = CHIP_MIN_WIDTH + icon_width + name_width;
```

### 13. Low (optional) — crates/workspace-model/src/history_shells.rs:29-51, 156-182; crates/workspace-model/src/history.rs:112-142 — Source: review-fm-ipl (CC-3)
**Verdict:** Confirmed. **Worth doing?** No: optional, splits of already-tested pure functions. **Status:** Skipped.
`shell_key` (~22 lines: file-stem extraction + classification), `switch_chip` (~26 lines:
modifier/letter parsing + cycling) and `from_jsonl` (~30 lines) are past ~15 lines doing two
steps each. Possible splits: `program_stem(program) -> (Vec<&str>, String)`;
`chip_letter(key, held) -> Option<char>`; `from_jsonl`'s per-line keep test as
`fn keeps(...)`. (`windows: bool` in `shell_key`/`shell_key_with` is the project's mandated
pure-function convention, not a CC-9 finding.)

```rust
    let mut parts: Vec<&str> = program.split(['/', '\\']).collect();
    let name = parts.pop().unwrap_or(program);
```

### 14. Low (optional) — crates/workspace-model/src/history.rs:168 — Source: review-fm-ipl (CC-1)
**Verdict:** Confirmed. **Worth doing?** Yes: clearer name. **Status:** Fixed.
`over` names a cap check: `over_cap` says what it is over.

```rust
        let over = *count > self.cap;
```

Fix: rename `over` -> `over_cap` (2 uses).
