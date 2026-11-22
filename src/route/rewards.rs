//! Common shapes of time-dependent rewards, for [`Route::reward`](super::Route::reward).
//!
//! Each takes the full reward, the finish time and the shape's parameters, and returns what
//! the task is worth at that finish time. [`deadline`] and [`linear_decay`] return `None`
//! when it is too late; wrap [`halving`] in `Some`. None of them grows with later finish
//! times.

/// The full `reward` up to and including `deadline`, and `None` after it.
///
/// ```
/// use task_allocation::route::rewards::deadline;
///
/// assert_eq!(deadline(10, 60, 60), Some(10));
/// assert_eq!(deadline(10, 61, 60), None);
/// ```
#[must_use]
pub const fn deadline(reward: u64, finish: u64, deadline: u64) -> Option<u64> {
    if finish <= deadline {
        Some(reward)
    } else {
        None
    }
}

/// The full `reward` up to `from`, falling linearly to zero at `until`, and `None` after
/// `until`. Rounds down. When `until` is not after `from`, this is a [`deadline`] at `from`.
///
/// ```
/// use task_allocation::route::rewards::linear_decay;
///
/// assert_eq!(linear_decay(100, 10, 10, 110), Some(100));
/// assert_eq!(linear_decay(100, 60, 10, 110), Some(50));
/// assert_eq!(linear_decay(100, 110, 10, 110), Some(0));
/// assert_eq!(linear_decay(100, 111, 10, 110), None);
/// ```
#[must_use]
pub fn linear_decay(reward: u64, finish: u64, from: u64, until: u64) -> Option<u64> {
    if finish <= from {
        return Some(reward);
    }

    if finish > until {
        return None;
    }

    let remaining = u128::from(until - finish);
    let span = u128::from(until - from);
    let scaled = u128::from(reward) * remaining / span;

    // The quotient is at most `reward`, so it always fits.
    Some(u64::try_from(scaled).unwrap_or(reward))
}

/// The full `reward` at time zero, halving every `half_life`, with linear steps in between,
/// and never `None`. Rounds down. A `half_life` of zero gives the full reward at time zero
/// and zero after.
///
/// This is the integer counterpart of CBBA's classic time-discounted reward `λ^t · reward`.
///
/// ```
/// use task_allocation::route::rewards::halving;
///
/// assert_eq!(halving(100, 0, 10), 100);
/// assert_eq!(halving(100, 10, 10), 50);
/// assert_eq!(halving(100, 15, 10), 37);
/// assert_eq!(halving(100, 20, 10), 25);
/// ```
#[must_use]
pub fn halving(reward: u64, finish: u64, half_life: u64) -> u64 {
    if half_life == 0 {
        return if finish == 0 { reward } else { 0 };
    }

    let halvings = finish / half_life;
    if halvings >= 64 {
        return 0;
    }

    let high = reward >> halvings;
    let low = high / 2;
    let into = u128::from(finish % half_life);
    let drop = (u128::from(high - low) * into).div_ceil(u128::from(half_life));

    // The drop is at most `high - low`, so it always fits.
    high - u64::try_from(drop).unwrap_or(high - low)
}
