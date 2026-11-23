//! Changing a running team: new tasks, finished tasks, and agents that leave.
//!
//! Every change keeps the bundles that are not affected. Agents bid on a new task with the
//! room they have left, so with full bundles it waits for room, or for a [`Replan`] that
//! releases some. Every agent has to apply the same change, in the same order, so that task
//! identifiers stay the same everywhere; meanwhile, messages are merged over the tasks both
//! sides know.
//!
//! [`Replan`]: super::Replan

use super::{Agent, Claim};
use crate::id::{AgentId, TaskId};
use crate::valuation::Valuation;

impl<V: Valuation<TaskId>> Agent<V> {
    /// Adds a task after the existing ones and returns its identifier; the next bundle phase
    /// may bid on it. Existing bundles stay as they are.
    pub fn add_task(&mut self) -> TaskId {
        let task = TaskId(self.claims.len());
        self.claims.push(Claim::NONE);
        self.retired.push(false);
        self.tasks.push(task);

        task
    }

    /// Retires `task`, because it is done or cancelled: nobody wins it any more, and this
    /// agent never bids on it again. If this agent holds it, it gives it up together with the
    /// tasks it added after it, whose gains assumed it. Returns whether this agent held it.
    ///
    /// # Panics
    ///
    /// Panics when `task` is not one of this agent's tasks.
    pub fn retire_task(&mut self, task: TaskId) -> bool {
        let held = self.give_up(task);
        self.retired[task.index()] = true;
        self.claims[task.index()] = Claim::NONE;
        self.tasks.retain(|&candidate| candidate != task);

        held
    }

    /// Records that `agent` left the team: its claims are void, now and when relayed later,
    /// and its messages are ignored. Its tasks become free to bid on.
    ///
    /// # Panics
    ///
    /// Panics when `agent` is not part of the team.
    pub fn forget_agent(&mut self, agent: AgentId) {
        self.departed[agent.index()] = true;
        for claim in &mut self.claims {
            if claim.winner == Some(agent) {
                *claim = Claim::NONE;
            }
        }
    }

    /// Makes this agent leave the team: it gives up its whole bundle and bids no more. The
    /// others should [forget](Agent::forget_agent) it.
    pub fn leave(&mut self) {
        self.release_own(0);
        self.capacity = 0;
        self.left = true;
    }
}

impl<V> Agent<V> {
    /// Whether this agent left the team.
    #[must_use]
    pub const fn has_left(&self) -> bool {
        self.left
    }

    /// Whether this agent was told that `agent` left the team.
    ///
    /// # Panics
    ///
    /// Panics when `agent` is not part of the team.
    #[must_use]
    pub fn has_departed(&self, agent: AgentId) -> bool {
        self.departed[agent.index()]
    }
}
