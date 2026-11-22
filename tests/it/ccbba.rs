//! CCBBA over simulated networks: settled runs meet every coupled constraint, agree, and
//! never beat the constrained optimum.

use proptest::collection::vec;
use proptest::prelude::*;
use task_allocation::ccbba::{Agent, Constraints, Optimism};
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, Assignment, TaskId, Valuation};

use crate::bundle::gains_replay;
use crate::cbba::{SETTLE_TICKS, network};
use crate::support::{self, Instance, instance_with};

/// Small enough for the brute-force constrained optimum.
const MAX_AGENTS: usize = 3;

/// Patience for optimistic bids in the generated runs.
const OPTIMISM: Optimism = Optimism {
    timeout: 4,
    solo_attempts: 2,
    any_attempts: 4,
};

/// Random activities over `tasks` tasks: consecutive groups of two or three tasks, each
/// pair dependent one way, both ways, mutually exclusive, or unrelated.
fn constraints(tasks: usize) -> impl Strategy<Value = Constraints> {
    (vec(2_usize..=3, tasks), vec(0_u8..5, tasks * 9)).prop_map(move |(sizes, codes)| {
        let mut constraints = Constraints::new(tasks);
        let mut start = 0;
        let mut code = codes.into_iter();
        for size in sizes {
            if start + size > tasks {
                break;
            }

            let members: Vec<TaskId> = (start..start + size).map(TaskId).collect();
            let mut matrix = vec![vec![0_i8; size]; size];
            for q in 0..size {
                for u in q + 1..size {
                    let (forward, backward) = match code.next().unwrap_or(0) {
                        1 => (1, 0),
                        2 => (1, 1),
                        3 => (-1, -1),
                        _ => (0, 0),
                    };
                    matrix[q][u] = forward;
                    matrix[u][q] = backward;
                }
            }

            let rows: Vec<&[i8]> = matrix.iter().map(Vec::as_slice).collect();
            constraints
                .add_activity(&members, &rows)
                .expect("well-formed");
            start += size;
        }

        constraints
    })
}

/// An instance, constraints over its tasks, and a network for its agents.
fn scenario() -> impl Strategy<Value = (Instance, Constraints, Network)> {
    instance_with(MAX_AGENTS).prop_flat_map(|instance| {
        let (tasks, agents) = (instance.tasks, instance.agents.len());

        (Just(instance), constraints(tasks), network(agents))
    })
}

/// Runs CCBBA until the team settles; `None` when it does not within [`SETTLE_TICKS`] of
/// the last partition healing.
pub fn run(
    instance: &Instance,
    constraints: &Constraints,
    network: Network,
) -> Option<Simulation<Agent<support::Agent>>> {
    let team = instance.agents.len();
    let agents = instance
        .agents
        .iter()
        .cloned()
        .enumerate()
        .map(|(id, valuation)| {
            Agent::new(
                AgentId(id),
                team,
                instance.tasks,
                instance.capacity,
                valuation,
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

/// The best value of any assignment that meets `constraints` and the capacity, by trying
/// every holder for every task and every build order of every bundle.
fn constrained_optimum(instance: &Instance, constraints: &Constraints) -> u64 {
    let choices = instance.agents.len() + 1;
    let combinations = (0..instance.tasks).fold(1_usize, |total, _| total * choices);
    let mut best = 0;

    for code in 0..combinations {
        let mut holders = Vec::with_capacity(instance.tasks);
        let mut rest = code;
        for _ in 0..instance.tasks {
            holders.push(rest % choices);
            rest /= choices;
        }

        if let Some(value) = assignment_value(instance, constraints, &holders) {
            best = best.max(value);
        }
    }

    best
}

/// The value of giving task `t` to agent `holders[t] - 1` (zero: unassigned), with each
/// bundle built in its best order; `None` when that breaks a constraint or the capacity.
fn assignment_value(
    instance: &Instance,
    constraints: &Constraints,
    holders: &[usize],
) -> Option<u64> {
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

    let mut total = 0;
    for (index, agent) in instance.agents.iter().enumerate() {
        let tasks: Vec<TaskId> = (0..instance.tasks)
            .map(TaskId)
            .filter(|task| holders[task.index()] == index + 1)
            .collect();
        if tasks.len() > instance.capacity {
            return None;
        }

        total += best_build(agent, &tasks, &mut Vec::new())?;
    }

    Some(total)
}

/// The best total gain of building exactly `remaining` onto `path`, over every order;
/// `None` when no order is feasible.
fn best_build(agent: &support::Agent, remaining: &[TaskId], path: &mut Vec<TaskId>) -> Option<u64> {
    if remaining.is_empty() {
        return Some(0);
    }

    let mut best = None;
    for (index, &task) in remaining.iter().enumerate() {
        let Some(insertion) = agent.best_insertion(path, &task) else {
            continue;
        };

        let others: Vec<TaskId> = remaining
            .iter()
            .enumerate()
            .filter(|&(other, _)| other != index)
            .map(|(_, &task)| task)
            .collect();
        path.insert(insertion.position, task);
        let rest = best_build(agent, &others, path);
        path.remove(insertion.position);

        if let Some(rest) = rest {
            best = best.max(Some(insertion.gain + rest));
        }
    }

    best
}

/// Checks what a settled CCBBA team promises: constraints met, no conflicts, agreement on
/// holders and bids, and honest bids.
fn stable(
    simulation: &Simulation<Agent<support::Agent>>,
    constraints: &Constraints,
) -> Result<Assignment, TestCaseError> {
    let assignment = simulation.assignment();
    prop_assert!(assignment.is_conflict_free());
    prop_assert!(constraints.is_satisfied_by(&assignment));

    for agent in simulation.agents() {
        gains_replay(agent.valuation(), agent.bundle())?;
        for task in (0..constraints.tasks()).map(TaskId) {
            prop_assert_eq!(agent.winner(task), assignment.holder(task));
        }
    }

    Ok(assignment)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Settled runs meet every constraint and never beat the constrained optimum. CCBBA
    /// does not always settle; runs that do not are skipped.
    #[test]
    fn settled_runs_meet_the_constraints((instance, constraints, network) in scenario()) {
        let Some(simulation) = run(&instance, &constraints, network) else {
            return Ok(());
        };

        let assignment = stable(&simulation, &constraints)?;
        prop_assert!(assignment.value() <= constrained_optimum(&instance, &constraints));

        let mut simulation = simulation;
        for _ in 0..20 {
            simulation.step();
            prop_assert!(simulation.is_settled());
        }

        prop_assert_eq!(simulation.assignment(), assignment);
    }
}

/// CCBBA can cycle forever, even on a perfect network. Task 0 and task 2 depend on each
/// other, task 1 depends on task 0, and tasks 1 and 2 exclude each other. Agent 1 outbids
/// agent 0 on task 0 whenever task 2 has a winner, but only agent 0's bundle provides task
/// 2, behind task 0. Losing task 0, agent 0 releases task 2; agent 1 then gives task 0 up,
/// agent 0 rebuilds both, and the cycle repeats.
#[test]
fn coupled_tasks_can_cycle() {
    let site = |position, duration, deadline| support::Site {
        position,
        duration,
        deadline,
    };
    let instance = Instance {
        agents: vec![
            support::Agent::Coverage(support::Coverage {
                covers: vec![165, 11, 6],
                weights: vec![4, 15, 6, 10, 11, 9, 5, 7],
                allowed: 19_459_671,
            }),
            support::Agent::Courier(task_allocation::BestInsertion(support::Courier {
                start: 36,
                sites: vec![site(96, 16, 198), site(40, 7, 76), site(37, 12, 45)],
                rewards: vec![27, 8, 13],
            })),
        ],
        tasks: 3,
        capacity: 3,
    };
    let mut constraints = Constraints::new(3);
    constraints
        .add_activity(
            &[TaskId(0), TaskId(1), TaskId(2)],
            &[&[0, 1, 1], &[0, 0, -1], &[1, -1, 0]],
        )
        .unwrap();

    assert!(run(&instance, &constraints, Network::default()).is_none());
}
