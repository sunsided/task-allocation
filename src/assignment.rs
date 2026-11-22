//! The outcome of an allocation: one bundle per agent.

use alloc::vec::Vec;

use crate::bundle::Bundle;
use crate::id::{AgentId, TaskId};

/// Which agent holds which tasks: one [`Bundle`] per agent, indexed by [`AgentId`].
///
/// An assignment is *conflict-free* when no task is in two bundles. Every algorithm in this
/// crate returns conflict-free assignments; consensus algorithms reach one once their agents
/// agree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Assignment {
    /// The bundle of each agent, indexed by agent.
    bundles: Vec<Bundle<TaskId>>,
}

impl Assignment {
    /// An assignment from one bundle per agent, in agent order.
    #[must_use]
    pub const fn from_bundles(bundles: Vec<Bundle<TaskId>>) -> Self {
        Self { bundles }
    }

    /// An assignment of `agents` empty bundles.
    #[must_use]
    pub fn empty(agents: usize) -> Self {
        Self::from_bundles((0..agents).map(|_| Bundle::new()).collect())
    }

    /// The number of agents.
    #[must_use]
    pub fn agents(&self) -> usize {
        self.bundles.len()
    }

    /// The bundle of `agent`.
    ///
    /// # Panics
    ///
    /// Panics when `agent` is not part of the assignment.
    #[must_use]
    pub fn bundle(&self, agent: AgentId) -> &Bundle<TaskId> {
        &self.bundles[agent.index()]
    }

    /// The bundle of `agent`, to change it.
    ///
    /// # Panics
    ///
    /// Panics when `agent` is not part of the assignment.
    pub fn bundle_mut(&mut self, agent: AgentId) -> &mut Bundle<TaskId> {
        &mut self.bundles[agent.index()]
    }

    /// The bundles, in agent order.
    #[must_use]
    pub fn bundles(&self) -> &[Bundle<TaskId>] {
        &self.bundles
    }

    /// The agent holding `task`; `None` when no agent holds it. When several do, the one with
    /// the smallest identifier.
    #[must_use]
    pub fn holder(&self, task: TaskId) -> Option<AgentId> {
        self.bundles
            .iter()
            .position(|bundle| bundle.contains(&task))
            .map(AgentId)
    }

    /// The total value: the sum of the bundles' values, saturating at [`u64::MAX`].
    #[must_use]
    pub fn value(&self) -> u64 {
        self.bundles
            .iter()
            .fold(0, |total, bundle| total.saturating_add(bundle.value()))
    }

    /// Whether every task is held by at most one agent.
    #[must_use]
    pub fn is_conflict_free(&self) -> bool {
        let mut seen: Vec<TaskId> = self
            .bundles
            .iter()
            .flat_map(Bundle::tasks)
            .copied()
            .collect();
        let held = seen.len();
        seen.sort_unstable();
        seen.dedup();

        seen.len() == held
    }
}

/// Unit tests for [`Assignment`].
#[cfg(test)]
mod tests {
    use super::Assignment;
    use crate::id::{AgentId, TaskId};
    use crate::valuation::Insertion;

    /// Records `task` in the bundle of `agent` with gain `gain`, appended to the path.
    fn give(assignment: &mut Assignment, agent: usize, task: usize, gain: u64) {
        let bundle = assignment.bundle_mut(AgentId(agent));
        let position = bundle.len();
        bundle.insert(TaskId(task), Insertion { position, gain });
    }

    /// Holders, values and conflicts follow the bundles.
    #[test]
    fn reports_holders_and_value() {
        let mut assignment = Assignment::empty(2);
        give(&mut assignment, 0, 1, 4);
        give(&mut assignment, 1, 0, 3);

        assert_eq!(assignment.agents(), 2);
        assert_eq!(assignment.bundles().len(), 2);
        assert_eq!(assignment.holder(TaskId(1)), Some(AgentId(0)));
        assert_eq!(assignment.holder(TaskId(2)), None);
        assert_eq!(assignment.value(), 7);
        assert!(assignment.is_conflict_free());

        give(&mut assignment, 1, 1, 2);
        assert!(!assignment.is_conflict_free());
    }
}
