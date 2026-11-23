//! The thesis table, [`Rules::Thesis2012`](super::Rules::Thesis2012): Table 4.1 of Luke
//! Johnson's 2012 MIT thesis, which renumbers the 2011 table and marks some of its actions
//! with † and ‡.
//!
//! The thesis does not define the marks. Its remarks on the rows describe pushing the
//! winning bid's time past the losing one's: "we make sure that the winning bid has a later
//! time than the losing bid, if it does we do not change the time, if it does not, we update
//! the time to be ε greater than the losing time". Read that way, † on an update stamps the
//! sender's claim past the receiver's, and ‡ stamps the receiver's own entry past the
//! sender's. Both readings are inferred.
//!
//! Everything else is the 2011 table, and rows keep their 2011 numbers. As tabulated, the
//! thesis also has the receiver keep its entry, with ‡, when the sender knows of no winner
//! and the receiver believes the sender wins (2011 row 32), where 2011 updates. Its text
//! disagrees with its own table there, and in three more rows, which this module follows
//! as tabulated:
//!
//! - 2011 row 20: the text ignores an older message; the table updates.
//! - 2011 row 30: the text trusts the later time; the table updates.
//! - 2011 row 32: the text calls it "the propagation step for a dropped bid"; the table
//!   keeps the receiver's entry.
//! - 2011 row 34: the text trusts the later time and rebroadcasts; the table stays silent.

use super::{Broadcast, Change, Decision};

/// The thesis's decision, given the 2011 table's `decision`.
pub(super) const fn decide(decision: Decision) -> Decision {
    let change = match (decision.row, decision.change) {
        (1 | 17, Change::Update) => {
            return Decision {
                change: Change::UpdatePast,
                broadcast: Broadcast::Own,
                row: decision.row,
            };
        }
        (3 | 19, Change::UpdateTime) | (13 | 15 | 31, _) => Change::KeepPast,
        (32, _) => {
            return Decision {
                change: Change::KeepPast,
                broadcast: Broadcast::Own,
                row: 32,
            };
        }
        (_, change) => change,
    };

    Decision { change, ..decision }
}

/// Unit tests for the marked rows.
#[cfg(test)]
mod tests;
