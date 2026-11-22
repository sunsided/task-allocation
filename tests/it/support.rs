//! Generated valuations with diminishing marginal gains, by construction.

use proptest::collection::vec;
use proptest::prelude::*;
use task_allocation::{BestInsertion, Insertion, PathScore, TaskId, Valuation};

/// The most tasks a generated instance has; keeps the exhaustive optimum fast.
pub const MAX_TASKS: usize = 6;

/// The most agents a generated instance has.
pub const MAX_AGENTS: usize = 3;

/// A task on a line, with a duration and a deadline, both in minutes.
#[derive(Clone, Copy, Debug)]
pub struct Site {
    /// Position on the line, in meters.
    pub position: u64,
    /// Minutes the task takes once the agent is there.
    pub duration: u64,
    /// Minutes after the start by which the task must be done.
    pub deadline: u64,
}

/// An agent that covers items: each task covers a set of weighted items, and a task is worth
/// the weight of the items it covers that the agent's other tasks do not.
///
/// Coverage is submodular, so gains diminish. Tasks outside `allowed` are infeasible.
#[derive(Clone, Debug)]
pub struct Coverage {
    /// For each task, the items it covers, as a bit mask over `weights`.
    pub covers: Vec<u16>,
    /// The weight of each item.
    pub weights: Vec<u64>,
    /// Which tasks the agent can take at all, as a bit mask over tasks.
    pub allowed: u32,
}

impl Valuation<TaskId> for Coverage {
    fn marginal_score(&self, bundle: &[TaskId], task: &TaskId) -> Option<u64> {
        if self.allowed & (1 << task.index()) == 0 {
            return None;
        }

        let covered = bundle
            .iter()
            .fold(0_u16, |mask, held| mask | self.covers[held.index()]);
        let fresh = self.covers[task.index()] & !covered;

        Some(
            self.weights
                .iter()
                .enumerate()
                .filter(|(item, _)| fresh & (1 << item) != 0)
                .map(|(_, weight)| weight)
                .sum(),
        )
    }
}

/// An agent that drives along a line to timed sites and earns a fixed reward for each one it
/// finishes by the deadline.
///
/// Constant rewards with deadlines diminish: adding a task only delays the others.
#[derive(Clone, Debug)]
pub struct Courier {
    /// Where the agent starts, in meters.
    pub start: u64,
    /// The sites, indexed by task.
    pub sites: Vec<Site>,
    /// The reward for each task the agent finishes in time.
    pub rewards: Vec<u64>,
}

impl PathScore<TaskId> for Courier {
    fn path_score(&self, path: &[TaskId]) -> Option<u64> {
        let mut at = self.start;
        let mut clock = 0;
        let mut total = 0;

        for task in path {
            let site = self.sites[task.index()];
            clock += at.abs_diff(site.position) + site.duration;
            if clock > site.deadline {
                return None;
            }

            at = site.position;
            total += self.rewards[task.index()];
        }

        Some(total)
    }
}

/// Either kind of generated agent.
#[derive(Clone, Debug)]
pub enum Agent {
    /// An order-independent coverage valuation.
    Coverage(Coverage),
    /// A path-dependent courier, inserting each task at its best position.
    Courier(BestInsertion<Courier>),
}

impl Valuation<TaskId> for Agent {
    fn marginal_score(&self, bundle: &[TaskId], task: &TaskId) -> Option<u64> {
        match self {
            Self::Coverage(coverage) => coverage.marginal_score(bundle, task),
            Self::Courier(courier) => courier.marginal_score(bundle, task),
        }
    }

    fn best_insertion(&self, path: &[TaskId], task: &TaskId) -> Option<Insertion> {
        match self {
            Self::Coverage(coverage) => coverage.best_insertion(path, task),
            Self::Courier(courier) => courier.best_insertion(path, task),
        }
    }
}

/// A generated instance: agents, the number of tasks, and the per-agent capacity.
#[derive(Clone, Debug)]
pub struct Instance {
    /// The agents, indexed by agent.
    pub agents: Vec<Agent>,
    /// The number of tasks.
    pub tasks: usize,
    /// The most tasks one agent may hold.
    pub capacity: usize,
}

impl Instance {
    /// Every task identifier, in order.
    pub fn task_ids(&self) -> Vec<TaskId> {
        (0..self.tasks).map(TaskId).collect()
    }
}

/// A coverage agent over `tasks` tasks.
fn coverage(tasks: usize) -> impl Strategy<Value = Coverage> {
    (
        vec(any::<u16>().prop_map(|mask| mask & 0xff), tasks),
        vec(1_u64..20, 8),
        any::<u32>(),
    )
        .prop_map(|(covers, weights, allowed)| Coverage {
            covers,
            weights,
            allowed: allowed | 1,
        })
}

/// A courier agent over `sites`.
fn courier(sites: Vec<Site>) -> impl Strategy<Value = BestInsertion<Courier>> {
    let tasks = sites.len();

    (0_u64..100, vec(1_u64..30, tasks)).prop_map(move |(start, rewards)| {
        BestInsertion(Courier {
            start,
            sites: sites.clone(),
            rewards,
        })
    })
}

/// Sites on a line from 0 to 100 meters, with deadlines up to five hours.
fn sites(tasks: usize) -> impl Strategy<Value = Vec<Site>> {
    vec(
        (0_u64..100, 1_u64..20, 20_u64..300).prop_map(|(position, duration, deadline)| Site {
            position,
            duration,
            deadline,
        }),
        tasks,
    )
}

/// An agent of either kind over `sites`.
fn agent(sites: Vec<Site>) -> impl Strategy<Value = Agent> {
    let tasks = sites.len();

    prop_oneof![
        coverage(tasks).prop_map(Agent::Coverage),
        courier(sites).prop_map(Agent::Courier),
    ]
}

/// A random instance whose agents all use [`Coverage`]: submodular set functions, for which
/// the greedy approximation bounds hold.
pub fn coverage_instance() -> impl Strategy<Value = Instance> {
    (1..=MAX_TASKS, 1..=MAX_AGENTS)
        .prop_flat_map(|(tasks, agents)| (Just(tasks), Just(agents), 1..=tasks))
        .prop_flat_map(|(tasks, agents, capacity)| {
            (
                vec(coverage(tasks).prop_map(Agent::Coverage), agents),
                Just(tasks),
                Just(capacity),
            )
        })
        .prop_map(|(agents, tasks, capacity)| Instance {
            agents,
            tasks,
            capacity,
        })
}

/// A random instance with up to [`MAX_AGENTS`] agents and [`MAX_TASKS`] tasks.
pub fn instance() -> impl Strategy<Value = Instance> {
    instance_with(MAX_AGENTS)
}

/// A random instance with up to `max_agents` agents and [`MAX_TASKS`] tasks.
pub fn instance_with(max_agents: usize) -> impl Strategy<Value = Instance> {
    (1..=MAX_TASKS, 1..=max_agents)
        .prop_flat_map(|(tasks, agents)| (Just(tasks), Just(agents), 1..=tasks))
        .prop_flat_map(|(tasks, agents, capacity)| {
            sites(tasks).prop_flat_map(move |sites| {
                (vec(agent(sites), agents), Just(tasks), Just(capacity))
            })
        })
        .prop_map(|(agents, tasks, capacity)| Instance {
            agents,
            tasks,
            capacity,
        })
}
