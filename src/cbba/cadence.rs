//! When an agent broadcasts: every round, or only after its winner tables change.
//!
//! CBBA as published has every agent broadcast its state every round. Most of those
//! messages repeat what the neighbors already know: once a team is close to agreement, few
//! entries change per round. [`Cadence::OnChange`] skips a round's broadcast when the
//! agent's winners and bids are the same as in its last message, following the trigger of
//! Sao, Ho, Bhore and Sujit, "Event Driven CBBA with Reduced Communication" (ICUAS 2025,
//! <https://arxiv.org/abs/2509.06481>), which analyzes it for synchronous rounds.
//!
//! The trigger looks at winners and bids only, not at the clocks: every message advances
//! its sender's clock, so two neighbors that answered each other's clock changes would
//! never fall silent.
//!
//! A skipped broadcast is never repeated by itself. A lost message, or a neighbor that was
//! out of reach when the tables last changed, then stays uninformed until something changes
//! again; `refresh` sends the unchanged state at least every that many rounds, so that a
//! team on a lossy or partitioned network still settles.

use super::Agent;
use crate::id::TaskId;
use crate::valuation::Valuation;

/// When an agent broadcasts its state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Cadence {
    /// Every round, as CBBA is published.
    #[default]
    EveryRound,

    /// Only in rounds where the agent's winners or bids differ from its last message, and,
    /// with `refresh`, at least every that many rounds.
    OnChange {
        /// The most rounds between two broadcasts, after which the unchanged state is sent
        /// anyway; `None` sets no limit, which only suits networks that lose no messages
        /// and do not partition. One broadcasts every round.
        refresh: Option<u64>,
    },
}

impl<V: Valuation<TaskId>> Agent<V> {
    /// This agent, broadcasting as `cadence` says.
    ///
    /// # Panics
    ///
    /// Panics when `cadence` refreshes after zero rounds.
    #[must_use]
    pub const fn with_cadence(mut self, cadence: Cadence) -> Self {
        assert!(
            !matches!(cadence, Cadence::OnChange { refresh: Some(0) }),
            "the refresh interval must be positive"
        );
        self.cadence = cadence;
        self
    }

    /// Whether this round calls for a broadcast under the agent's [`Cadence`]. Call it once
    /// per round, after the bundle phase; a round without a broadcast counts towards the
    /// refresh interval.
    pub fn broadcast_due(&mut self) -> bool {
        let Cadence::OnChange { refresh } = self.cadence else {
            return true;
        };

        let due = self.claims != self.sent
            || refresh.is_some_and(|rounds| self.silent_rounds + 1 >= rounds);
        if !due {
            self.silent_rounds += 1;
        }

        due
    }
}
