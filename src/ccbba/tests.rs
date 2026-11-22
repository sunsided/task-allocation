//! Unit tests for [`Agent`] and [`Constraints`].

use alloc::vec::Vec;

use super::{Agent, ConstraintError, Constraints, Optimism};
use crate::assignment::Assignment;
use crate::id::{AgentId, TaskId};
use crate::valuation::{Insertion, Valuation};

/// Values each task at a fixed reward, for any number of tasks.
#[derive(Clone)]
struct Fixed(Vec<u64>);

impl Valuation<TaskId> for Fixed {
    fn marginal_score(&self, _bundle: &[TaskId], task: &TaskId) -> Option<u64> {
        Some(self.0[task.index()]).filter(|&reward| reward > 0)
    }
}

/// Generous optimism: long enough to wait, a few attempts.
const OPTIMISM: Optimism = Optimism {
    timeout: 3,
    solo_attempts: 2,
    any_attempts: 3,
};

/// Agent `id` of a team of `agents`, valuing tasks at `rewards` under `constraints`.
fn agent(id: usize, agents: usize, rewards: &[u64], constraints: &Constraints) -> Agent<Fixed> {
    Agent::new(
        AgentId(id),
        agents,
        rewards.len(),
        rewards.len(),
        Fixed(rewards.to_vec()),
        constraints.clone(),
        OPTIMISM,
    )
}

/// The tasks in `agent`'s bundle, in the order it added them.
fn bundle_of(agent: &Agent<Fixed>) -> Vec<usize> {
    agent.bundle().tasks().map(|task| task.index()).collect()
}

/// Task 1 depends on task 0, but not the other way round.
fn unilateral() -> Constraints {
    let mut constraints = Constraints::new(2);
    constraints
        .add_activity(&[TaskId(0), TaskId(1)], &[&[0, 1], &[0, 0]])
        .unwrap();

    constraints
}

/// Malformed activities are rejected and leave the constraints unchanged.
#[test]
fn activities_are_validated() {
    let mut constraints = Constraints::new(3);

    assert_eq!(
        constraints.add_activity(&[TaskId(5)], &[&[0]]),
        Err(ConstraintError::UnknownTask(TaskId(5)))
    );
    assert_eq!(
        constraints.add_activity(&[TaskId(0), TaskId(0)], &[&[0, 0], &[0, 0]]),
        Err(ConstraintError::DuplicateTask(TaskId(0)))
    );
    assert_eq!(
        constraints.add_activity(&[TaskId(0), TaskId(1)], &[&[0, 1]]),
        Err(ConstraintError::Shape)
    );
    assert_eq!(
        constraints.add_activity(&[TaskId(0)], &[&[1]]),
        Err(ConstraintError::Diagonal)
    );
    assert_eq!(
        constraints.add_activity(&[TaskId(0), TaskId(1)], &[&[0, 2], &[0, 0]]),
        Err(ConstraintError::UnsupportedCode(2))
    );
    assert_eq!(constraints, Constraints::new(3));

    constraints
        .add_activity(&[TaskId(0), TaskId(1)], &[&[0, -1], &[0, 0]])
        .unwrap();
    assert_eq!(
        constraints.add_activity(&[TaskId(1)], &[&[0]]),
        Err(ConstraintError::DuplicateTask(TaskId(1)))
    );
    assert!(constraints.excludes(TaskId(1), TaskId(0)));
    assert!(!constraints.excludes(TaskId(1), TaskId(2)));
}

/// Satisfaction checks requirements and exclusions of assigned tasks only.
#[test]
fn satisfaction_follows_the_assignment() {
    let constraints = unilateral();
    let mut assignment = Assignment::empty(1);
    let take = |assignment: &mut Assignment, task: usize| {
        let bundle = assignment.bundle_mut(AgentId(0));
        let position = bundle.len();
        bundle.insert(TaskId(task), Insertion { position, gain: 1 });
    };

    assert!(constraints.is_satisfied_by(&assignment));
    take(&mut assignment, 1);
    assert!(!constraints.is_satisfied_by(&assignment));
    take(&mut assignment, 0);
    assert!(constraints.is_satisfied_by(&assignment));
}

/// A pessimistic task is only bid on once its dependency has a winner.
#[test]
fn pessimistic_tasks_wait_for_their_dependencies() {
    let constraints = unilateral();
    let mut alone = agent(0, 1, &[0, 9], &constraints);
    let mut helper = agent(0, 1, &[1, 9], &constraints);

    alone.build_bundle();
    helper.build_bundle();
    assert_eq!(bundle_of(&alone), Vec::<usize>::new());
    assert_eq!(bundle_of(&helper), [0]);

    helper.build_bundle();
    assert_eq!(bundle_of(&helper), [0, 1]);
}

/// An optimistic task waits for its partner, gives up after the timeout, and stops trying
/// once its attempts are used up.
#[test]
fn optimistic_tasks_time_out() {
    let mut constraints = Constraints::new(2);
    constraints
        .add_activity(&[TaskId(0), TaskId(1)], &[&[0, 1], &[1, 0]])
        .unwrap();
    let mut lonely = agent(0, 1, &[9, 0], &constraints);

    let mut held = Vec::new();
    for _ in 0..20 {
        lonely.build_bundle();
        held.push(lonely.bundle().len());
    }

    assert_eq!(held[0], 1);
    assert_eq!(held[19], 0);
    assert!(!lonely.can_bid());
}

/// An agent does not bid on a task when an excluded task's winner outbids it, and gives a
/// task up when that happens later.
#[test]
fn exclusions_need_the_higher_bid() {
    let mut constraints = Constraints::new(2);
    constraints
        .add_activity(&[TaskId(0), TaskId(1)], &[&[0, -1], &[-1, 0]])
        .unwrap();
    let mut low = agent(0, 2, &[5, 0], &constraints);
    let mut high = agent(1, 2, &[0, 9], &constraints);

    low.build_bundle();
    high.build_bundle();
    assert_eq!(bundle_of(&low), [0]);

    low.receive(&high.message());
    low.build_bundle();
    assert_eq!(bundle_of(&low), Vec::<usize>::new());
    assert_eq!(low.winner(TaskId(0)), None);
    assert_eq!(low.winner(TaskId(1)), Some(AgentId(1)));
}

/// Constraints for a different task count are a caller bug.
#[test]
#[should_panic(expected = "must cover exactly the tasks")]
fn constraints_must_match_the_tasks() {
    let _ = agent(0, 1, &[1, 1, 1], &unilateral());
}

/// The agent exposes its identity, valuation, constraints and view of the winners.
#[test]
fn accessors_report_the_agents_state() {
    let constraints = unilateral();
    let mut helper = agent(1, 2, &[4, 9], &constraints);
    helper.build_bundle();

    assert_eq!(helper.id(), AgentId(1));
    assert_eq!(helper.valuation().0, [4, 9]);
    assert_eq!(helper.constraints(), &constraints);
    assert_eq!(helper.winning_bid(TaskId(0)), 4);
    assert_eq!(
        helper.inner.valuation().marginal_score(&[], &TaskId(0)),
        Some(4)
    );
    assert!(!helper.inner.give_up(TaskId(1)));
}

/// A task released along with an earlier one starts counting its wait from zero when it is
/// added again.
#[test]
fn released_tasks_forget_how_long_they_waited() {
    let mut constraints = Constraints::new(3);
    constraints
        .add_activity(&[TaskId(1), TaskId(2)], &[&[0, 1], &[1, 0]])
        .unwrap();
    let mut agent = agent(0, 2, &[9, 5, 0], &constraints);

    agent.build_bundle();
    agent.build_bundle();
    assert_eq!(bundle_of(&agent), [0, 1]);
    assert_eq!(agent.waited[1], 1);

    agent.inner.give_up(TaskId(0));
    agent.build_bundle();
    assert_eq!(agent.waited[1], 0);
}

/// Timing matrices are validated, and limits only bind dependent tasks.
#[test]
fn timing_is_validated() {
    let mut constraints = Constraints::new(3);
    let tasks = [TaskId(0), TaskId(1)];
    let depends: [&[i8]; 2] = [&[0, 1], &[0, 0]];
    let unrelated: [&[i8]; 2] = [&[0, 0], &[0, 0]];

    assert_eq!(
        constraints.add_timed_activity(&tasks, &depends, &[&[Some(0)]]),
        Err(ConstraintError::TimingShape)
    );
    assert_eq!(
        constraints.add_timed_activity(&tasks, &depends, &[&[Some(1), None], &[None, None]]),
        Err(ConstraintError::TimingDiagonal)
    );
    assert_eq!(
        constraints.add_timed_activity(&tasks, &unrelated, &[&[None, Some(5)], &[None, None]]),
        Err(ConstraintError::TimingWithoutDependency)
    );

    constraints
        .add_timed_activity(&tasks, &depends, &[&[None, Some(-2)], &[Some(9), None]])
        .unwrap();
    constraints.set_opening(TaskId(1), 40);

    assert_eq!(constraints.opening(TaskId(1)), 40);
    assert_eq!(
        constraints.window(TaskId(1), TaskId(0), 10),
        (Some(12), Some(19))
    );
    assert_eq!(constraints.window(TaskId(0), TaskId(1), 10), (None, None));
    assert_eq!(constraints.window(TaskId(2), TaskId(0), 10), (None, None));
    assert!(constraints.starts_agree(TaskId(1), 15, TaskId(0), 10));
    assert!(!constraints.starts_agree(TaskId(1), 11, TaskId(0), 10));
    assert!(constraints.starts_agree(TaskId(2), 0, TaskId(0), 99));
}

/// Every error explains itself.
#[test]
fn errors_display() {
    let errors = [
        ConstraintError::UnknownTask(TaskId(4)),
        ConstraintError::DuplicateTask(TaskId(1)),
        ConstraintError::Shape,
        ConstraintError::Diagonal,
        ConstraintError::UnsupportedCode(2),
        ConstraintError::TimingShape,
        ConstraintError::TimingDiagonal,
        ConstraintError::TimingWithoutDependency,
    ];

    for error in errors {
        assert_ne!(alloc::format!("{error}"), "");
    }

    assert_eq!(
        alloc::format!("{}", ConstraintError::UnknownTask(TaskId(4))),
        "task 4 is not part of the task set"
    );
}
