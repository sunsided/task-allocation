//! Coupled assignment constraints between tasks, grouped into activities.

use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use crate::assignment::Assignment;
use crate::id::TaskId;

/// Assignment constraints between tasks: which tasks need others, and which exclude each
/// other.
///
/// Tasks are grouped into *activities*. Within an activity, a dependency matrix `D` says how
/// each pair of tasks relates, as in Table I of Whitten, Choi, Johnson and How (2011). Entry
/// `D[q][u]`, with `q` and `u` positions in the activity's task list, is:
///
/// - `1`: task `u` depends on task `q`; `u` may only be assigned if `q` is;
/// - `0`: no constraint;
/// - `-1`: `q` and `u` are mutually exclusive; at most one of them may be assigned.
///
/// Two tasks that depend on each other must be assigned together or not at all. The diagonal
/// is zero. Tasks outside every activity are unconstrained. The paper's codes above one
/// ("either of these tasks") are not supported.
///
/// # Examples
///
/// ```
/// use task_allocation::TaskId;
/// use task_allocation::ccbba::Constraints;
///
/// // A strike (task 0) needs a sensor (task 1), and the sensor is useless without it.
/// // Task 2 excludes both.
/// let mut constraints = Constraints::new(3);
/// constraints
///     .add_activity(
///         &[TaskId(0), TaskId(1), TaskId(2)],
///         &[&[0, 1, -1], &[1, 0, -1], &[-1, -1, 0]],
///     )
///     .unwrap();
///
/// assert_eq!(constraints.requirements(TaskId(0)).collect::<Vec<_>>(), [TaskId(1)]);
/// assert!(constraints.excludes(TaskId(2), TaskId(0)));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Constraints {
    /// For each task, its activity and its position in it; `None` for unconstrained tasks.
    membership: Vec<Option<(usize, usize)>>,

    /// The activities: their tasks, in matrix order, and their dependency matrices.
    activities: Vec<Activity>,
}

/// A group of coupled tasks with its dependency matrix.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Activity {
    /// The tasks, in the order of the matrix's rows and columns.
    tasks: Vec<TaskId>,

    /// `dependency[q][u]`: how the tasks at positions `q` and `u` relate; see [`Constraints`].
    dependency: Vec<Vec<i8>>,
}

/// Why an activity was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConstraintError {
    /// A task identifier is not below the task count.
    UnknownTask(TaskId),

    /// A task appears twice in one activity, or already belongs to another activity.
    DuplicateTask(TaskId),

    /// The matrix is not square with one row per task.
    Shape,

    /// A diagonal entry is not zero.
    Diagonal,

    /// An entry is not `-1`, `0` or `1`.
    UnsupportedCode(i8),
}

impl fmt::Display for ConstraintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTask(task) => write!(f, "{task} is not part of the task set"),
            Self::DuplicateTask(task) => write!(f, "{task} is already part of an activity"),
            Self::Shape => f.write_str("the dependency matrix needs one row and column per task"),
            Self::Diagonal => f.write_str("the dependency matrix needs a zero diagonal"),
            Self::UnsupportedCode(code) => {
                write!(f, "dependency code {code} is not supported; use -1, 0 or 1")
            }
        }
    }
}

impl core::error::Error for ConstraintError {}

impl Constraints {
    /// No constraints between tasks `0..tasks`.
    #[must_use]
    pub fn new(tasks: usize) -> Self {
        Self {
            membership: vec![None; tasks],
            activities: Vec::new(),
        }
    }

    /// Adds an activity: `tasks`, related by `dependency` as described on [`Constraints`].
    ///
    /// # Errors
    ///
    /// Returns a [`ConstraintError`] when a task is unknown or already in an activity, when
    /// the matrix is not square with one row per task, when its diagonal is not zero, or when
    /// an entry is not `-1`, `0` or `1`. Nothing is added then.
    pub fn add_activity(
        &mut self,
        tasks: &[TaskId],
        dependency: &[&[i8]],
    ) -> Result<(), ConstraintError> {
        self.check_tasks(tasks)?;
        check_matrix(tasks.len(), dependency)?;

        let activity = self.activities.len();
        for (position, task) in tasks.iter().enumerate() {
            self.membership[task.index()] = Some((activity, position));
        }

        self.activities.push(Activity {
            tasks: tasks.to_vec(),
            dependency: dependency.iter().map(|row| row.to_vec()).collect(),
        });

        Ok(())
    }

    /// Checks that every task is known, and in no activity yet, nor twice in `tasks`.
    fn check_tasks(&self, tasks: &[TaskId]) -> Result<(), ConstraintError> {
        for (index, &task) in tasks.iter().enumerate() {
            let Some(membership) = self.membership.get(task.index()) else {
                return Err(ConstraintError::UnknownTask(task));
            };

            if membership.is_some() || tasks[..index].contains(&task) {
                return Err(ConstraintError::DuplicateTask(task));
            }
        }

        Ok(())
    }

    /// The number of tasks the constraints cover.
    #[must_use]
    pub fn tasks(&self) -> usize {
        self.membership.len()
    }

    /// The tasks `task` depends on: it may only be assigned if all of them are.
    pub fn requirements(&self, task: TaskId) -> impl Iterator<Item = TaskId> + '_ {
        self.related(task)
            .filter(|&(_, toward, _)| toward == 1)
            .map(|(other, _, _)| other)
    }

    /// Whether `a` and `b` are mutually exclusive.
    #[must_use]
    pub fn excludes(&self, a: TaskId, b: TaskId) -> bool {
        self.related(a)
            .any(|(other, toward, away)| other == b && (toward == -1 || away == -1))
    }

    /// The tasks mutually exclusive with `task`.
    pub fn exclusions(&self, task: TaskId) -> impl Iterator<Item = TaskId> + '_ {
        self.related(task)
            .filter(|&(_, toward, away)| toward == -1 || away == -1)
            .map(|(other, _, _)| other)
    }

    /// Whether `task` depends on a task that also depends on it. Agents may bid on such a task
    /// optimistically, before its partners have winners.
    #[must_use]
    pub fn is_mutual(&self, task: TaskId) -> bool {
        self.related(task)
            .any(|(_, toward, away)| toward == 1 && away == 1)
    }

    /// Every other task in `task`'s activity, with the matrix entries `D[other][task]` and
    /// `D[task][other]`.
    fn related(&self, task: TaskId) -> impl Iterator<Item = (TaskId, i8, i8)> + '_ {
        let membership = self.membership.get(task.index()).copied().flatten();

        membership
            .into_iter()
            .flat_map(move |(activity, position)| {
                let activity = &self.activities[activity];
                activity
                    .tasks
                    .iter()
                    .enumerate()
                    .filter(move |&(other, _)| other != position)
                    .map(move |(other, &other_task)| {
                        (
                            other_task,
                            activity.dependency[other][position],
                            activity.dependency[position][other],
                        )
                    })
            })
    }

    /// Whether `assignment` meets every constraint: each assigned task's requirements are
    /// assigned too, and no two mutually exclusive tasks are both assigned.
    #[must_use]
    pub fn is_satisfied_by(&self, assignment: &Assignment) -> bool {
        let assigned = |task: TaskId| assignment.holder(task).is_some();

        (0..self.tasks())
            .map(TaskId)
            .filter(|&task| assigned(task))
            .all(|task| {
                self.requirements(task).all(assigned) && !self.exclusions(task).any(assigned)
            })
    }
}

/// Checks that `dependency` is a square matrix of side `size` with a zero diagonal and
/// entries `-1`, `0` or `1`.
fn check_matrix(size: usize, dependency: &[&[i8]]) -> Result<(), ConstraintError> {
    if dependency.len() != size || dependency.iter().any(|row| row.len() != size) {
        return Err(ConstraintError::Shape);
    }

    for (q, row) in dependency.iter().enumerate() {
        for (u, &code) in row.iter().enumerate() {
            if !(-1..=1).contains(&code) {
                return Err(ConstraintError::UnsupportedCode(code));
            }

            if q == u && code != 0 {
                return Err(ConstraintError::Diagonal);
            }
        }
    }

    Ok(())
}
