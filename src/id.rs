//! Dense identifiers for agents and tasks.
//!
//! Both are plain indices: agent `AgentId(3)` is the fourth agent of a team and task
//! `TaskId(0)` the first task of a task set. Algorithms keep per-task and per-agent tables
//! indexed by them, so identifiers should be small and contiguous, starting at zero.

use core::fmt;

/// Identifies an agent by its index in the team, starting at zero.
///
/// The order matters: when two agents bid the same score for a task, the agent with the
/// smaller identifier wins. Every algorithm in this crate breaks ties this way, so all agents
/// agree on the winner without exchanging anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AgentId(pub usize);

/// Identifies a task by its index in the task set, starting at zero.
///
/// When an agent values two tasks the same, it picks the one with the smaller identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(pub usize);

impl AgentId {
    /// The agent's index into per-agent tables.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

impl TaskId {
    /// The task's index into per-task tables.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "agent {}", self.0)
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "task {}", self.0)
    }
}

/// Unit tests for the identifier types.
#[cfg(test)]
mod tests {
    use super::{AgentId, TaskId};
    use alloc::string::ToString;

    /// Identifiers print with their kind, so log lines stay unambiguous.
    #[test]
    fn display_names_the_kind() {
        assert_eq!(AgentId(2).to_string(), "agent 2");
        assert_eq!(TaskId(7).to_string(), "task 7");
    }

    /// Smaller identifiers order first, which is what tie-breaking relies on.
    #[test]
    fn order_follows_the_index() {
        assert!(AgentId(1) < AgentId(2));
        assert_eq!(TaskId(4).index(), 4);
    }
}
