//! CBBA with bid warping, for valuations whose marginal gains do not diminish: runs settle
//! on conflict-free assignments that all agents agree on, with bids that never grow along a
//! bundle and that nobody can beat.

use proptest::collection::vec;
use proptest::prelude::*;
use task_allocation::cbba::{Agent, Replan};
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, BestInsertion, PathScore, TaskId, Valuation, check_diminishing};

use crate::cbba::{MAX_TEAM, SETTLE_TICKS, network};

/// The reward per task visited, as in Sec. VI.A of Johnson et al. (2017).
const REWARD: u64 = 100;

/// The fuel cost per unit of distance traveled, as in Sec. VI.A of Johnson et al. (2017).
const FUEL: u64 = 10;

/// The most tasks a generated route instance has.
const MAX_TASKS: usize = 8;

/// The highest coordinate on the grid tasks and agents stand on; with [`REWARD`] and
/// [`FUEL`], a task up to ten steps away is worth visiting.
const GRID: u64 = 8;

/// A point on the grid.
type Point = (u64, u64);

/// The Manhattan distance between `a` and `b`.
const fn distance(a: Point, b: Point) -> u64 {
    a.0.abs_diff(b.0) + a.1.abs_diff(b.1)
}

/// An agent that visits task sites from its start, earning [`REWARD`] per site and paying
/// [`FUEL`] per unit of distance: the route score of Eq. 33 in Johnson et al. (2017).
///
/// A site next to one already visited costs little extra travel, so it can be worth more
/// after another task than on its own: gains do not diminish.
#[derive(Clone, Debug)]
struct Route {
    /// Where the agent starts.
    start: Point,

    /// The site of each task, indexed by task.
    sites: Vec<Point>,
}

impl PathScore<TaskId> for Route {
    fn path_score(&self, path: &[TaskId]) -> Option<u64> {
        let (traveled, _) = path.iter().fold((0, self.start), |(traveled, at), task| {
            let site = self.sites[task.index()];
            (traveled + distance(at, site), site)
        });
        let earned = REWARD * u64::try_from(path.len()).expect("short path");

        earned.checked_sub(FUEL * traveled)
    }
}

/// Agents and tasks for a route instance.
#[derive(Clone, Debug)]
struct Instance {
    /// Each agent's valuation.
    agents: Vec<BestInsertion<Route>>,

    /// The number of tasks.
    tasks: usize,

    /// The most tasks an agent holds.
    capacity: usize,
}

impl Instance {
    /// Whether some agent's gains fail to diminish on these tasks.
    fn is_non_diminishing(&self) -> bool {
        let tasks: Vec<TaskId> = (0..self.tasks).map(TaskId).collect();

        self.agents
            .iter()
            .any(|agent| check_diminishing(agent, &tasks, self.capacity).is_err())
    }
}

/// A point on the grid.
fn point() -> impl Strategy<Value = Point> {
    (0..=GRID, 0..=GRID)
}

/// Up to [`MAX_TEAM`] agents and [`MAX_TASKS`] task sites on the grid, with a capacity of
/// two to four.
fn instance() -> impl Strategy<Value = Instance> {
    (
        vec(point(), 1..=MAX_TASKS),
        vec(point(), 2..=MAX_TEAM),
        2_usize..=4,
    )
        .prop_map(|(sites, starts, capacity)| Instance {
            tasks: sites.len(),
            agents: starts
                .into_iter()
                .map(|start| {
                    BestInsertion(Route {
                        start,
                        sites: sites.clone(),
                    })
                })
                .collect(),
            capacity,
        })
}

/// A CBBA team valuing `tasks` tasks as `agents` say, each holding up to `capacity`,
/// replanning as `replan` says, and warping its bids when `warping` is set.
fn team<V: Valuation<TaskId> + Clone>(
    agents: &[V],
    tasks: usize,
    capacity: usize,
    replan: Replan,
    warping: bool,
) -> Vec<Agent<V>> {
    agents
        .iter()
        .cloned()
        .enumerate()
        .map(|(id, valuation)| {
            let agent = Agent::new(AgentId(id), agents.len(), tasks, capacity, valuation)
                .with_replan(replan);
            if warping {
                agent.with_bid_warping()
            } else {
                agent
            }
        })
        .collect()
}

/// Runs `agents` on `network` until they settle; `None` when they do not settle within
/// [`SETTLE_TICKS`] of the last partition healing.
fn run<V: Valuation<TaskId>>(
    agents: Vec<Agent<V>>,
    network: Network,
) -> Option<Simulation<Agent<V>>> {
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

/// Checks what a settled warping team promises: no conflicts, agreement on winners and
/// bids, bids that never grow along a bundle, and no agent with room left able to beat a
/// winner with its warped bid.
fn stable<V: Valuation<TaskId>>(
    simulation: &Simulation<Agent<V>>,
    capacity: usize,
) -> Result<(), TestCaseError> {
    let assignment = simulation.assignment();
    prop_assert!(assignment.is_conflict_free());

    for agent in simulation.agents() {
        let bids: Vec<u64> = agent.bundle().entries().map(|(_, bid)| bid).collect();
        prop_assert!(bids.windows(2).all(|pair| pair[0] >= pair[1]), "{:?}", bids);
        prop_assert!(agent.bundle().len() <= capacity);

        for (task, winner) in agent.winners().enumerate() {
            let task = TaskId(task);
            prop_assert_eq!(winner, assignment.holder(task));

            let recorded = winner.and_then(|holder| {
                assignment
                    .bundle(holder)
                    .entries()
                    .find(|(held, _)| **held == task)
                    .map(|(_, bid)| bid)
            });
            prop_assert_eq!(agent.winning_bid(task), recorded.unwrap_or(0));
        }

        prop_assert!(agent.bundle().len() >= capacity || !agent.can_bid());
    }

    Ok(())
}

/// Example 2's valuation: the agent values its `first` task at `T`, the other at `T - δ`,
/// and whichever it adds second at `T + δ`, with `T = 10` and `δ = 1`.
#[derive(Clone, Debug)]
struct Example2 {
    /// The task this agent values most on its own.
    first: TaskId,
}

impl Valuation<TaskId> for Example2 {
    fn marginal_score(&self, path: &[TaskId], task: &TaskId) -> Option<u64> {
        match (path.is_empty(), *task == self.first) {
            (true, true) => Some(10),
            (true, false) => Some(9),
            (false, _) => Some(11),
        }
    }
}

/// The two agents of Example 2 (Fig. 3 of Johnson et al., 2017), each preferring a
/// different task first, both able to hold both tasks.
fn example2(warping: bool) -> Vec<Agent<Example2>> {
    let agents = [Example2 { first: TaskId(0) }, Example2 { first: TaskId(1) }];

    team(&agents, 2, 2, Replan::Keep, warping)
}

/// Example 2 with warping: each agent bids `T` on both tasks, agent 0 wins the tie on
/// both, and agent 1 cannot beat `T` with a single task at `T - δ`.
#[test]
fn example2_settles_with_warping() {
    let mut simulation = Simulation::new(example2(true), Network::default());
    simulation.run_until_settled(100).expect("settles");

    let first = &simulation.agents()[0];
    assert_eq!(
        first.bundle().entries().collect::<Vec<_>>(),
        [(&TaskId(0), 10), (&TaskId(1), 10)]
    );
    assert!(simulation.agents()[1].bundle().is_empty());
}

/// Example 2 without warping settles here too, unlike the paper's cycle, with agent 0
/// holding both tasks and bidding more on its second task than on its first. CBBA's
/// consensus differs from the paper's information model; the two-courier cycle below is
/// the regression that does cycle here.
#[test]
fn example2_settles_without_warping_with_growing_bids() {
    let mut simulation = Simulation::new(example2(false), Network::default());
    simulation.run_until_settled(100).expect("settles");

    let first = &simulation.agents()[0];
    assert_eq!(
        first.bundle().entries().collect::<Vec<_>>(),
        [(&TaskId(0), 10), (&TaskId(1), 11)]
    );
}

/// Two couriers and two neighboring sites, the route version of Example 2. Agent 0 values
/// site 0 at 60 and site 1 after it at 70; agent 1 values site 0 at 80 and site 1 after it
/// at 70. Without warping, agent 0's growing second bid keeps unsettling the team, whether
/// agents keep or rebuild their bundles, even without losses or delays.
fn two_couriers(replan: Replan, warping: bool) -> Simulation<Agent<BestInsertion<Route>>> {
    let sites = vec![(3, 6), (6, 6)];
    let agents: Vec<_> = [(3, 2), (4, 7)]
        .into_iter()
        .map(|start| {
            BestInsertion(Route {
                start,
                sites: sites.clone(),
            })
        })
        .collect();

    Simulation::new(team(&agents, 2, 2, replan, warping), Network::default())
}

/// Without warping, the two couriers never settle, whether they keep or rebuild bundles.
#[test]
fn two_couriers_cycle_without_warping() {
    for replan in [Replan::Keep, Replan::Full] {
        let mut simulation = two_couriers(replan, false);
        assert_eq!(simulation.run_until_settled(1_000), None, "{replan:?}");
    }
}

/// With warping, agent 0 bids 60 on site 1 too, loses both sites to agent 1, and the team
/// settles at once.
#[test]
fn two_couriers_settle_with_warping() {
    for replan in [Replan::Keep, Replan::Full] {
        let mut simulation = two_couriers(replan, true);
        simulation.run_until_settled(100).expect("settles");

        assert!(simulation.agents()[0].bundle().is_empty(), "{replan:?}");
        assert_eq!(
            simulation.agents()[1]
                .bundle()
                .entries()
                .collect::<Vec<_>>(),
            [(&TaskId(0), 80), (&TaskId(1), 70)],
            "{replan:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Route scores, whose gains do not diminish, settle on stable warped assignments over
    /// lossy, delayed and partitioned networks, whether agents keep their bundles or
    /// rebuild them every round as the paper does.
    #[test]
    fn route_scores_settle(
        (instance, network) in instance().prop_filter("gains diminish", Instance::is_non_diminishing)
            .prop_flat_map(|instance| {
                let agents = instance.agents.len();
                (Just(instance), network(agents))
            }),
        full in any::<bool>(),
    ) {
        let replan = if full { Replan::Full } else { Replan::Keep };
        let agents = team(&instance.agents, instance.tasks, instance.capacity, replan, true);
        let Some(simulation) = run(agents, network) else {
            return Err(TestCaseError::fail("did not settle"));
        };

        stable(&simulation, instance.capacity)?;
    }
}
