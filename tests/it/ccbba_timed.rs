//! CCBBA with temporal constraints over simulated networks: settled runs meet every
//! constraint, their starts respect every timing limit, every holder can execute its
//! announced starts on its own route, and no settled run beats a brute-force optimum whose
//! timing check is a simple temporal network from temporal-control.

use proptest::collection::vec;
use proptest::prelude::*;
use task_allocation::ccbba::{Constraints, Optimism, TimedAgent};
use task_allocation::route::{Route, rewards};
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, TaskId};

use temporal_control::{Budget, Stn, Ticks, Verdict};

use crate::cbba::{SETTLE_TICKS, network};

/// The most agents in a generated team.
const MAX_AGENTS: usize = 3;

/// The most tasks in a generated instance.
const MAX_TASKS: usize = 6;

/// Patience for optimistic bids in the generated runs.
const OPTIMISM: Optimism = Optimism {
    timeout: 4,
    solo_attempts: 2,
    any_attempts: 4,
};

/// A task on a line: where, how long, and by when it must be done.
#[derive(Clone, Copy, Debug)]
struct Site {
    /// Position on the line.
    at: u64,
    /// Time the task takes.
    duration: u64,
    /// Latest finish.
    deadline: u64,
}

/// An agent on a line at unit speed, paid a fixed reward per task done by its deadline.
#[derive(Clone, Debug)]
struct Line {
    /// Where the agent starts.
    origin: u64,
    /// The sites, indexed by task.
    sites: Vec<Site>,
    /// The reward per task, indexed by task.
    rewards: Vec<u64>,
}

impl Route for Line {
    type Place = u64;

    fn origin(&self) -> u64 {
        self.origin
    }

    fn place(&self, task: TaskId) -> u64 {
        self.sites[task.index()].at
    }

    fn travel(&self, from: &u64, to: &u64) -> Option<u64> {
        Some(from.abs_diff(*to))
    }

    fn duration(&self, task: TaskId) -> u64 {
        self.sites[task.index()].duration
    }

    fn reward(&self, task: TaskId, finish: u64) -> Option<u64> {
        let reward = self.rewards[task.index()];

        rewards::deadline(reward, finish, self.sites[task.index()].deadline).filter(|_| reward > 0)
    }
}

/// A generated instance: the agents' routes and the per-agent capacity.
#[derive(Clone, Debug)]
struct Instance {
    /// The agents, indexed by agent.
    lines: Vec<Line>,
    /// The number of tasks.
    tasks: usize,
    /// The most tasks one agent may hold.
    capacity: usize,
}

/// Random instances of up to [`MAX_AGENTS`] agents and [`MAX_TASKS`] tasks on a shared set
/// of sites.
fn instance() -> impl Strategy<Value = Instance> {
    instance_of(MAX_TASKS, MAX_AGENTS)
}

/// Random instances of up to `max_agents` agents and `max_tasks` tasks on a shared set of
/// sites.
fn instance_of(max_tasks: usize, max_agents: usize) -> impl Strategy<Value = Instance> {
    (1..=max_tasks, 1..=max_agents)
        .prop_flat_map(|(tasks, agents)| {
            let sites = vec((0_u64..60, 1_u64..10, 20_u64..200), tasks).prop_map(|sites| {
                sites
                    .into_iter()
                    .map(|(at, duration, deadline)| Site {
                        at,
                        duration,
                        deadline,
                    })
                    .collect::<Vec<_>>()
            });

            (
                sites,
                vec((0_u64..60, vec(0_u64..20, tasks)), agents),
                1..=tasks,
            )
        })
        .prop_map(|(sites, agents, capacity)| Instance {
            tasks: sites.len(),
            lines: agents
                .into_iter()
                .map(|(origin, rewards)| Line {
                    origin,
                    sites: sites.clone(),
                    rewards,
                })
                .collect(),
            capacity,
        })
}

/// Random timed activities over `tasks` tasks: consecutive groups of two or three, each pair
/// dependent one way, both ways, mutually exclusive or unrelated, with random timing limits
/// between dependent pairs.
fn constraints(tasks: usize) -> impl Strategy<Value = Constraints> {
    (
        vec(2_usize..=3, tasks),
        vec(0_u8..5, tasks * 9),
        vec(proptest::option::of(-20_i64..40), tasks * 18),
    )
        .prop_map(move |(sizes, codes, limits)| build_constraints(tasks, &sizes, &codes, &limits))
}

/// Builds constraints from drawn group sizes, pair codes and timing limits.
fn build_constraints(
    tasks: usize,
    sizes: &[usize],
    codes: &[u8],
    limits: &[Option<i64>],
) -> Constraints {
    let mut constraints = Constraints::new(tasks);
    let mut code = codes.iter();
    let mut limit = limits.iter();
    let mut start = 0;
    for &size in sizes {
        if start + size > tasks {
            break;
        }

        let members: Vec<TaskId> = (start..start + size).map(TaskId).collect();
        let mut dependency = vec![vec![0_i8; size]; size];
        let mut timing = vec![vec![None; size]; size];
        for q in 0..size {
            timing[q][q] = Some(0);
            for u in q + 1..size {
                let (forward, backward) = match code.next().copied().unwrap_or(0) {
                    1 => (1, 0),
                    2 => (1, 1),
                    3 => (-1, -1),
                    _ => (0, 0),
                };
                dependency[q][u] = forward;
                dependency[u][q] = backward;
                if forward == 1 || backward == 1 {
                    timing[q][u] = limit.next().copied().flatten();
                    timing[u][q] = limit.next().copied().flatten();
                }
            }
        }

        let dependency_rows: Vec<&[i8]> = dependency.iter().map(Vec::as_slice).collect();
        let timing_rows: Vec<&[Option<i64>]> = timing.iter().map(Vec::as_slice).collect();
        constraints
            .add_timed_activity(&members, &dependency_rows, &timing_rows)
            .expect("well-formed");
        start += size;
    }

    constraints
}

/// An instance, timed constraints over its tasks, and a network for its agents.
fn scenario() -> impl Strategy<Value = (Instance, Constraints, Network)> {
    with_constraints(instance())
}

/// `instances`, each with timed constraints over its tasks and a network for its agents.
fn with_constraints(
    instances: impl Strategy<Value = Instance>,
) -> impl Strategy<Value = (Instance, Constraints, Network)> {
    instances.prop_flat_map(|instance| {
        let (tasks, agents) = (instance.tasks, instance.lines.len());

        (Just(instance), constraints(tasks), network(agents))
    })
}

/// Runs timed CCBBA until the team settles; `None` when it does not within
/// [`SETTLE_TICKS`] of the last partition healing.
fn run(
    instance: &Instance,
    constraints: &Constraints,
    network: Network,
) -> Option<Simulation<TimedAgent<Line>>> {
    let team = instance.lines.len();
    let agents = instance
        .lines
        .iter()
        .cloned()
        .enumerate()
        .map(|(id, line)| {
            TimedAgent::new(
                AgentId(id),
                team,
                instance.tasks,
                instance.capacity,
                line,
                constraints.clone(),
                OPTIMISM,
            )
        })
        .collect();
    let healed = network
        .partitions
        .iter()
        .map(|partition| partition.until)
        .max()
        .unwrap_or(0);
    let mut simulation = Simulation::new(agents, network);
    simulation.run_until_settled(healed + SETTLE_TICKS)?;

    Some(simulation)
}

/// Checks that `agent` can execute its bundle on its own route at the starts the team
/// agreed on: it arrives in time for each start, and finishes each task by its deadline.
fn executable(agent: &TimedAgent<Line>) -> Result<(), TestCaseError> {
    let route = agent.route();
    let mut at = route.origin();
    let mut clock = route.departure();
    for &task in agent.bundle().path() {
        let start = agent
            .start(task)
            .ok_or_else(|| TestCaseError::fail("held task without a start"))?;
        let arrival = clock + route.travel(&at, &route.place(task)).expect("on the line");
        prop_assert!(
            arrival <= start,
            "{:?} starts {} before arriving at {}",
            task,
            start,
            arrival
        );

        let finish = start + route.duration(task);
        prop_assert!(
            route.reward(task, finish).is_some(),
            "{:?} finishes too late",
            task
        );
        at = route.place(task);
        clock = finish;
    }

    Ok(())
}

/// Checks what a settled timed team promises: constraints met, agreement, timing limits
/// respected between every pair of assigned starts, and executable bundles.
fn stable(
    simulation: &Simulation<TimedAgent<Line>>,
    constraints: &Constraints,
    tasks: usize,
) -> Result<(), TestCaseError> {
    let assignment = simulation.assignment();
    prop_assert!(assignment.is_conflict_free());
    prop_assert!(constraints.is_satisfied_by(&assignment));

    let first = &simulation.agents()[0];
    for agent in simulation.agents() {
        executable(agent)?;
        for task in (0..tasks).map(TaskId) {
            prop_assert_eq!(agent.winner(task), assignment.holder(task));
            prop_assert_eq!(agent.start(task), first.start(task));
        }
    }

    for task in (0..tasks).map(TaskId) {
        for other in (0..tasks).map(TaskId) {
            if let (Some(start), Some(other_start)) = (first.start(task), first.start(other)) {
                prop_assert!(
                    constraints.starts_agree(task, start, other, other_start),
                    "{:?} at {} and {:?} at {}",
                    task,
                    start,
                    other,
                    other_start
                );
            }
        }
    }

    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Settled runs meet every constraint, timing included, with executable bundles, and
    /// stay settled. Runs that do not settle are skipped; `most_timed_runs_settle` bounds
    /// how many.
    #[test]
    fn settled_timed_runs_meet_the_constraints((instance, constraints, network) in scenario()) {
        let Some(mut simulation) = run(&instance, &constraints, network) else {
            return Ok(());
        };

        stable(&simulation, &constraints, instance.tasks)?;
        let assignment = simulation.assignment();
        for _ in 0..20 {
            simulation.step();
            prop_assert!(simulation.is_settled());
        }

        prop_assert_eq!(simulation.assignment(), assignment);
    }
}

/// Most generated timed runs settle: at least 95% of 2,000 drawn with a fixed seed.
#[test]
fn most_timed_runs_settle() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    let mut runner = TestRunner::deterministic();
    let scenarios = scenario();
    let settled = (0..2_000)
        .filter(|_| {
            let (instance, constraints, network) = scenarios
                .new_tree(&mut runner)
                .expect("generates")
                .current();
            run(&instance, &constraints, network).is_some()
        })
        .count();

    assert!(settled >= 1_900, "only {settled} of 2000 runs settled");
}

/// The most tasks for the brute-force optimum.
const ORACLE_TASKS: usize = 4;

/// The best value any feasible plan reaches: every task given to at most one agent within
/// capacity, every dependency met and no two excluded tasks taken, and start times, found by
/// a simple temporal network, at which every agent can walk its tasks in some order by their
/// deadlines while every timing limit holds. Rewards are fixed, so a plan is worth the sum
/// of its tasks' rewards.
fn timed_optimum(instance: &Instance, constraints: &Constraints) -> u64 {
    let choices = instance.lines.len() + 1;
    let combinations = (0..instance.tasks).fold(1_usize, |total, _| total * choices);
    let mut best = 0;

    for code in 0..combinations {
        let holders = decode(code, choices, instance.tasks);
        let Some(value) = holders_value(instance, constraints, &holders) else {
            continue;
        };

        if value > best && some_order_fits(instance, constraints, &holders) {
            best = value;
        }
    }

    best
}

/// The holder of each task for combination `code`: zero for none, `agent + 1` otherwise.
fn decode(code: usize, choices: usize, tasks: usize) -> Vec<usize> {
    let mut rest = code;

    (0..tasks)
        .map(|_| {
            let holder = rest % choices;
            rest /= choices;
            holder
        })
        .collect()
}

/// The value of giving each task to `holders` (zero: unassigned), ignoring time; `None` when
/// that breaks a dependency, an exclusion or a capacity.
fn holders_value(instance: &Instance, constraints: &Constraints, holders: &[usize]) -> Option<u64> {
    let assigned = |task: TaskId| holders[task.index()] > 0;
    for task in (0..instance.tasks)
        .map(TaskId)
        .filter(|&task| assigned(task))
    {
        if !constraints.requirements(task).all(assigned)
            || constraints.exclusions(task).any(assigned)
        {
            return None;
        }
    }

    let mut value = 0;
    for agent in 1..=instance.lines.len() {
        let held: Vec<usize> = (0..instance.tasks)
            .filter(|&task| holders[task] == agent)
            .collect();
        if held.len() > instance.capacity {
            return None;
        }

        value += held
            .iter()
            .map(|&task| instance.lines[agent - 1].rewards[task])
            .sum::<u64>();
    }

    Some(value)
}

/// Whether some visiting order of every agent's tasks admits start times meeting the
/// routes, deadlines and timing limits.
fn some_order_fits(instance: &Instance, constraints: &Constraints, holders: &[usize]) -> bool {
    let sets: Vec<Vec<usize>> = (1..=instance.lines.len())
        .map(|agent| {
            (0..instance.tasks)
                .filter(|&task| holders[task] == agent)
                .collect()
        })
        .collect();
    let mut orders = vec![Vec::new(); sets.len()];

    choose_orders(instance, constraints, &sets, &mut orders, 0)
}

/// Tries every order of agent `agent`'s tasks, and of every later agent's, with `orders`
/// fixed for the earlier agents.
fn choose_orders(
    instance: &Instance,
    constraints: &Constraints,
    sets: &[Vec<usize>],
    orders: &mut [Vec<usize>],
    agent: usize,
) -> bool {
    if agent == sets.len() {
        return schedule_exists(instance, constraints, orders);
    }

    permutations(&sets[agent]).into_iter().any(|order| {
        orders[agent] = order;
        choose_orders(instance, constraints, sets, orders, agent + 1)
    })
}

/// Every order of `items`.
fn permutations(items: &[usize]) -> Vec<Vec<usize>> {
    if items.is_empty() {
        return vec![Vec::new()];
    }

    let mut all = Vec::new();
    for (index, &first) in items.iter().enumerate() {
        let rest: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|&(other, _)| other != index)
            .map(|(_, &item)| item)
            .collect();
        for mut tail in permutations(&rest) {
            tail.insert(0, first);
            all.push(tail);
        }
    }

    all
}

/// Seconds as temporal-control ticks.
fn ticks(value: i128) -> Ticks {
    Ticks(i64::try_from(value).expect("small times"))
}

/// Whether start times exist at which every agent walks `orders` on its line by the
/// deadlines while every timing limit between assigned tasks holds.
fn schedule_exists(instance: &Instance, constraints: &Constraints, orders: &[Vec<usize>]) -> bool {
    let mut stn = Stn::new();
    let origin = stn.add_time_point().expect("fits");
    let mut starts = vec![None; instance.tasks];
    for order in orders {
        for &task in order {
            starts[task] = Some(stn.add_time_point().expect("fits"));
        }
    }

    for (agent, order) in orders.iter().enumerate() {
        let line = &instance.lines[agent];
        let mut previous: Option<usize> = None;
        for &task in order {
            let site = line.sites[task];
            let start = starts[task].expect("assigned");
            let reach = match previous {
                None => i128::from(line.origin.abs_diff(site.at)),
                Some(before) => {
                    let gap = line.sites[before].duration + line.sites[before].at.abs_diff(site.at);
                    stn.add_constraint(
                        starts[before].expect("assigned"),
                        start,
                        Some(ticks(i128::from(gap))),
                        None,
                    )
                    .expect("exists");
                    0
                }
            };
            let latest = i128::from(site.deadline) - i128::from(site.duration);
            stn.add_constraint(origin, start, Some(ticks(reach)), Some(ticks(latest)))
                .expect("exists");
            previous = Some(task);
        }
    }

    for task in 0..instance.tasks {
        for other in 0..instance.tasks {
            let (Some(start), Some(other_start)) = (starts[task], starts[other]) else {
                continue;
            };

            // The window `other` imposes on `task`, relative to `other` starting at zero.
            let (from, until) = constraints.window(TaskId(task), TaskId(other), 0);
            if from.is_some() || until.is_some() {
                stn.add_constraint(other_start, start, from.map(ticks), until.map(ticks))
                    .expect("exists");
            }
        }
    }

    matches!(
        stn.check_consistency(&Budget::UNLIMITED),
        Ok(Verdict::Consistent(_))
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Settled timed runs never beat the best plan that meets every constraint, timing
    /// included. The oracle's timing check also accepts every settled plan, which checks its
    /// encoding against the plans CCBBA actually executes.
    #[test]
    fn settled_timed_runs_never_beat_the_optimum(
        (instance, constraints, network) in with_constraints(instance_of(ORACLE_TASKS, MAX_AGENTS)),
    ) {
        let Some(simulation) = run(&instance, &constraints, network) else {
            return Ok(());
        };

        let assignment = simulation.assignment();
        let orders: Vec<Vec<usize>> = assignment
            .bundles()
            .iter()
            .map(|bundle| bundle.path().iter().map(|task| task.index()).collect())
            .collect();
        prop_assert!(
            schedule_exists(&instance, &constraints, &orders),
            "the oracle rejects the settled plan {:?}",
            orders
        );

        let value = assignment.value();
        let best = timed_optimum(&instance, &constraints);
        prop_assert!(value <= best, "{} above the optimum {}", value, best);
    }
}
