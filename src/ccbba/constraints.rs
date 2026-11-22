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

    /// For each task, the opening of its time window: the earliest it may start, in the
    /// time unit of the agents' routes. Zero unless set.
    openings: Vec<u64>,
}

/// A group of coupled tasks with its dependency matrix.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Activity {
    /// The tasks, in the order of the matrix's rows and columns.
    tasks: Vec<TaskId>,

    /// `dependency[q][u]`: how the tasks at positions `q` and `u` relate; see [`Constraints`].
    dependency: Vec<Vec<i8>>,

    /// `timing[q][u]`: the most task `q` may start after task `u` starts; `None` for no
    /// limit. Finite only between dependent tasks.
    timing: Vec<Vec<Option<i64>>>,
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

    /// The timing matrix is not square with one row per task.
    TimingShape,

    /// A diagonal entry of the timing matrix is not zero or unbounded.
    TimingDiagonal,

    /// The timing matrix limits two tasks that do not depend on each other in either
    /// direction; the paper defines no windows or release rules for those.
    TimingWithoutDependency,
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
            Self::TimingShape => f.write_str("the timing matrix needs one row and column per task"),
            Self::TimingDiagonal => f.write_str("the timing matrix needs a zero diagonal"),
            Self::TimingWithoutDependency => {
                f.write_str("timing limits apply only between dependent tasks")
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
            openings: vec![0; tasks],
        }
    }

    /// Adds an activity: `tasks`, related by `dependency` as described on [`Constraints`],
    /// without timing limits.
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
        let unbounded = vec![None; tasks.len()];
        let rows: Vec<&[Option<i64>]> = tasks.iter().map(|_| unbounded.as_slice()).collect();

        self.add_timed_activity(tasks, dependency, &rows)
    }

    /// Adds an activity whose dependent tasks must also start within limits of each other.
    ///
    /// `timing[q][u]` is the most task `q` may start after task `u` starts, in the time unit
    /// of the agents' routes; negative values make `q` start before `u`; `None` sets no
    /// limit. Together, `u` imposes on `q` the window `start(u) - timing[u][q]` to
    /// `start(u) + timing[q][u]`. This is the matrix `T` of Whitten, Choi, Johnson and How.
    ///
    /// # Errors
    ///
    /// As [`add_activity`](Constraints::add_activity), plus when the timing matrix is not
    /// square with one row per task, has a diagonal other than zero or `None`, or limits two
    /// tasks that do not depend on each other.
    ///
    /// # Examples
    ///
    /// The paper's strike activity: the strike (task 0) and the sensing (task 1) start within
    /// 20 seconds of each other, and the damage assessment (task 2) starts at least 180
    /// seconds after the strike and the sensing. Task 3, gathering intelligence instead,
    /// excludes all three.
    ///
    /// ```
    /// use task_allocation::TaskId;
    /// use task_allocation::ccbba::Constraints;
    ///
    /// let mut constraints = Constraints::new(4);
    /// constraints
    ///     .add_timed_activity(
    ///         &[TaskId(0), TaskId(1), TaskId(2), TaskId(3)],
    ///         &[&[0, 1, 1, -1], &[1, 0, 1, -1], &[0, 0, 0, -1], &[-1, -1, -1, 0]],
    ///         &[
    ///             &[Some(0), Some(20), Some(-180), None],
    ///             &[Some(20), Some(0), Some(-180), None],
    ///             &[None, None, Some(0), None],
    ///             &[None, None, None, Some(0)],
    ///         ],
    ///     )
    ///     .unwrap();
    ///
    /// // A strike starting at 100 lets the sensing start from 80 to 120, and the damage
    /// // assessment from 280 on.
    /// assert_eq!(constraints.window(TaskId(1), TaskId(0), 100), (Some(80), Some(120)));
    /// assert_eq!(constraints.window(TaskId(2), TaskId(0), 100), (Some(280), None));
    /// ```
    pub fn add_timed_activity(
        &mut self,
        tasks: &[TaskId],
        dependency: &[&[i8]],
        timing: &[&[Option<i64>]],
    ) -> Result<(), ConstraintError> {
        self.check_tasks(tasks)?;
        check_matrix(tasks.len(), dependency)?;
        check_timing(dependency, timing)?;

        let activity = self.activities.len();
        for (position, task) in tasks.iter().enumerate() {
            self.membership[task.index()] = Some((activity, position));
        }

        self.activities.push(Activity {
            tasks: tasks.to_vec(),
            dependency: dependency.iter().map(|row| row.to_vec()).collect(),
            timing: timing.iter().map(|row| row.to_vec()).collect(),
        });

        Ok(())
    }

    /// Sets the opening of `task`'s time window, the earliest it may start. Agents that
    /// conflict over two mutually dependent tasks compare how far into their windows they
    /// start, and the earlier one gives way.
    ///
    /// # Panics
    ///
    /// Panics when `task` is not part of the task set.
    pub fn set_opening(&mut self, task: TaskId, opening: u64) {
        self.openings[task.index()] = opening;
    }

    /// The opening of `task`'s time window; zero unless set.
    ///
    /// # Panics
    ///
    /// Panics when `task` is not part of the task set.
    #[must_use]
    pub fn opening(&self, task: TaskId) -> u64 {
        self.openings[task.index()]
    }

    /// The window `other`, starting at `start`, imposes on `task`: the earliest and the
    /// latest start, each `None` when unbounded. Only a task that depends on `other` gets a
    /// window; otherwise both are `None`. Bounds are not clamped at zero.
    #[must_use]
    pub fn window(&self, task: TaskId, other: TaskId, start: u64) -> (Option<i128>, Option<i128>) {
        let Some((after, before, depends)) = self.timing_between(task, other) else {
            return (None, None);
        };

        if !depends {
            return (None, None);
        }

        let start = i128::from(start);

        (
            before.map(|lead| start - i128::from(lead)),
            after.map(|lag| start + i128::from(lag)),
        )
    }

    /// Whether starts at `start` for `task` and `other_start` for `other` respect the
    /// timing limits between them, in both directions.
    #[must_use]
    pub fn starts_agree(&self, task: TaskId, start: u64, other: TaskId, other_start: u64) -> bool {
        let Some((after, before, _)) = self.timing_between(task, other) else {
            return true;
        };

        let (start, other_start) = (i128::from(start), i128::from(other_start));
        let late_enough = after.is_none_or(|lag| start <= other_start + i128::from(lag));
        let early_enough = before.is_none_or(|lead| other_start <= start + i128::from(lead));

        late_enough && early_enough
    }

    /// The other tasks in `task`'s activity; none for an unconstrained task.
    pub(crate) fn activity_tasks(&self, task: TaskId) -> impl Iterator<Item = TaskId> + '_ {
        self.related(task).map(|(other, _, _)| other)
    }

    /// Whether `task` depends on `other`.
    #[must_use]
    pub fn depends_on(&self, task: TaskId, other: TaskId) -> bool {
        self.requirements(task).any(|required| required == other)
    }

    /// `timing[task][other]`, `timing[other][task]`, and whether `task` depends on `other`;
    /// `None` when the two are not in the same activity.
    fn timing_between(
        &self,
        task: TaskId,
        other: TaskId,
    ) -> Option<(Option<i64>, Option<i64>, bool)> {
        let (activity, q) = self.membership.get(task.index()).copied().flatten()?;
        let (other_activity, u) = self.membership.get(other.index()).copied().flatten()?;
        if activity != other_activity || q == u {
            return None;
        }

        let activity = &self.activities[activity];

        Some((
            activity.timing[q][u],
            activity.timing[u][q],
            activity.dependency[u][q] == 1,
        ))
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

/// Checks that `timing` is a square matrix of the same side as `dependency`, with a zero or
/// unbounded diagonal, and finite only between tasks that depend on each other in some
/// direction.
fn check_timing(dependency: &[&[i8]], timing: &[&[Option<i64>]]) -> Result<(), ConstraintError> {
    let size = dependency.len();
    if timing.len() != size || timing.iter().any(|row| row.len() != size) {
        return Err(ConstraintError::TimingShape);
    }

    for (q, row) in timing.iter().enumerate() {
        for (u, &limit) in row.iter().enumerate() {
            if q == u {
                if limit.is_some_and(|limit| limit != 0) {
                    return Err(ConstraintError::TimingDiagonal);
                }

                continue;
            }

            let dependent = dependency[q][u] == 1 || dependency[u][q] == 1;
            if limit.is_some() && !dependent {
                return Err(ConstraintError::TimingWithoutDependency);
            }
        }
    }

    Ok(())
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
