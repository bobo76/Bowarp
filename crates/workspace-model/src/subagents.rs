//! The subagents an agent session runs, from its hooks. Each hook runs in its own process, so
//! events can arrive out of order: a subagent's tool event before its start, or the spawning
//! tool call (which carries the description) after the start it caused.

/// How long a spawn tool call's description waits for the subagent it starts.
const PENDING_FOR_MICROS: u64 = 60_000_000;
/// Most spawn descriptions kept waiting for their subagent.
const MAX_PENDING: usize = 8;
/// Most ended subagent ids remembered, so a late event cannot bring one back.
const MAX_FINISHED: usize = 32;

/// A subagent change reported by a hook. `id`s are the agent's own subagent ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubagentEvent {
    /// The main agent called its spawn tool with this description.
    Spawned { description: String },
    /// A subagent started, of the agent's own type (e.g. `Explore`).
    Started { id: String, kind: Option<String> },
    /// The subagent ended.
    Finished { id: String },
    /// The subagent ran a tool or otherwise moved on: it is working.
    Active { id: String },
    /// The subagent waits for the user (a permission dialog or a question).
    Waiting { id: String },
}

/// One running subagent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subagent {
    pub id: String,
    pub kind: Option<String>,
    pub description: Option<String>,
    pub waiting: bool,
    /// When its first event's hook started, in microseconds since the Unix epoch.
    started_at: u64,
    /// When the hook behind `waiting` started.
    changed_at: u64,
}

impl Subagent {
    /// What the sidebar shows: the description, else the type.
    pub fn label(&self) -> &str {
        self.description
            .as_deref()
            .or(self.kind.as_deref())
            .unwrap_or("Subagent")
    }
}

/// A session's running subagents, in start order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Subagents {
    running: Vec<Subagent>,
    /// Spawn descriptions not yet given to a subagent, with their hook start time.
    pending: Vec<(u64, String)>,
    finished: Vec<String>,
}

impl Subagents {
    pub fn running(&self) -> &[Subagent] {
        &self.running
    }

    pub fn is_empty(&self) -> bool {
        self.running.is_empty()
    }

    /// Apply an event whose hook started at `at`. Returns whether the running list changed.
    pub fn apply(&mut self, event: SubagentEvent, at: u64) -> bool {
        match event {
            SubagentEvent::Spawned { description } => self.spawned(description, at),
            SubagentEvent::Started { id, kind } => self.started(id, kind, at),
            SubagentEvent::Finished { id } => self.finished(id),
            SubagentEvent::Active { id } => self.set_waiting(id, false, at),
            SubagentEvent::Waiting { id } => self.set_waiting(id, true, at),
        }
    }

    /// Forget everything: the agent ended.
    pub fn clear(&mut self) -> bool {
        let had_running = !self.running.is_empty();
        *self = Self::default();
        had_running
    }

    fn spawned(&mut self, description: String, at: u64) -> bool {
        // The start it caused may have arrived first: the earliest one since, still unnamed.
        if let Some(subagent) = self
            .running
            .iter_mut()
            .filter(|subagent| subagent.description.is_none() && subagent.started_at >= at)
            .min_by_key(|subagent| subagent.started_at)
        {
            subagent.description = Some(description);
            return true;
        }
        if self.pending.len() == MAX_PENDING {
            self.pending.remove(0);
        }
        self.pending.push((at, description));
        false
    }

    fn started(&mut self, id: String, kind: Option<String>, at: u64) -> bool {
        if self.finished.contains(&id) {
            return false;
        }
        let (index, mut changed) = match self.running.iter().position(|s| s.id == id) {
            Some(index) => {
                // Its tool event came first.
                let subagent = &mut self.running[index];
                let named = subagent.kind.is_none() && kind.is_some();
                subagent.kind = subagent.kind.take().or(kind);
                subagent.started_at = subagent.started_at.min(at);
                (index, named)
            }
            None => {
                self.running.push(Subagent {
                    id,
                    kind,
                    description: None,
                    waiting: false,
                    started_at: at,
                    changed_at: at,
                });
                (self.running.len() - 1, true)
            }
        };
        changed |= self.take_description(index, at);
        changed
    }

    /// Give subagent `index`, when it has no description, the oldest recent spawn description
    /// from before `at`. Returns whether it got one.
    fn take_description(&mut self, index: usize, at: u64) -> bool {
        self.pending
            .retain(|(spawned_at, _)| spawned_at.saturating_add(PENDING_FOR_MICROS) >= at);
        if self.running[index].description.is_some() {
            return false;
        }
        let Some(position) = self
            .pending
            .iter()
            .position(|(spawned_at, _)| *spawned_at <= at)
        else {
            return false;
        };
        self.running[index].description = Some(self.pending.remove(position).1);
        true
    }

    fn finished(&mut self, id: String) -> bool {
        let before = self.running.len();
        self.running.retain(|subagent| subagent.id != id);
        if !self.finished.contains(&id) {
            if self.finished.len() == MAX_FINISHED {
                self.finished.remove(0);
            }
            self.finished.push(id);
        }
        self.running.len() != before
    }

    fn set_waiting(&mut self, id: String, waiting: bool, at: u64) -> bool {
        if self.finished.contains(&id) {
            return false;
        }
        let (index, created) = match self.running.iter().position(|s| s.id == id) {
            Some(index) => (index, false),
            None => {
                // Its start has not arrived yet.
                self.started(id, None, at);
                (self.running.len() - 1, true)
            }
        };
        let subagent = &mut self.running[index];
        if at < subagent.changed_at || subagent.waiting == waiting {
            return created;
        }
        subagent.waiting = waiting;
        subagent.changed_at = at;
        true
    }
}

#[cfg(test)]
mod tests;
