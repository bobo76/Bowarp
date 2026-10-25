# UX/UI decisions

## Keyboard page: may recording take a terminal control key (Ctrl+D, Ctrl+C, Alt+B)?
- Date: 2026-10-25, step: keymap-editor, raised by: REVIEW.md #1
- Context: workspace shortcuts match from inside the terminal, so a recorded Ctrl or Alt +
  letter silently stops reaching the shell or agent (EOF, interrupt, readline keys).
- Option A — Refuse: refuse Ctrl or Alt plus a letter alone with the warning toast already
  used for typing keys; recording continues. Pros: terminal keys can't be lost by a click;
  same rule as typing keys. Cons: blocks deliberate choices such as Ctrl+Q (still possible by
  hand in the settings file).
- Option B — Save, then warn: accept the key and show a toast that programs won't get it.
  Pros: keeps full freedom. Cons: the loss happens first and is easy to miss.
- Option C — Leave it: document it only. Pros: no work. Cons: users find out when Ctrl+D
  stops working.
- Decision: A, because a terminal key lost to one click is worse than the extra step to bind
  one by hand. Ctrl+digit, Ctrl+symbol, Ctrl+Shift/Ctrl+Alt + letter, Super and F-keys stay
  allowed. Decided by: user
