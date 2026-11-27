//! Unit tests for [`Quiet`].

use super::Quiet;

/// Unchanged views count up from the initial one; a change restarts the count, as does a
/// new task.
#[test]
fn counts_unchanged_phases() {
    let mut quiet = Quiet::new([1, 2].into_iter());
    assert_eq!(quiet.rounds(), 0);

    quiet.observe([1, 2].into_iter());
    quiet.observe([1, 2].into_iter());
    assert_eq!(quiet.rounds(), 2);

    quiet.observe([1, 3].into_iter());
    assert_eq!(quiet.rounds(), 0);

    quiet.observe([1, 3, 0].into_iter());
    assert_eq!(quiet.rounds(), 0);
}
