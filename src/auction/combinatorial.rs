//! Combinatorial auctions: bids on whole bundles, and exact winner determination.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec;
use alloc::vec::Vec;

use crate::assignment::Assignment;
use crate::bundle::Bundle;
use crate::id::{AgentId, TaskId};
use crate::valuation::Valuation;

/// One agent's bid on a bundle of tasks: the bundle, built in its best order, and its value.
///
/// Bids of one agent are exclusive (*XOR bids*): the agent wins at most one of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleBid {
    /// The bidding agent.
    pub agent: AgentId,

    /// The tasks bid on, with their path and recorded gains. The bid's value is
    /// [`Bundle::value`].
    pub bundle: Bundle<TaskId>,
}

impl BundleBid {
    /// The bid's value: what the bundle is worth to the agent.
    #[must_use]
    pub fn value(&self) -> u64 {
        self.bundle.value()
    }
}

/// The `limit` most valuable bundles of at most `max_size` tasks out of `0..tasks`, as bids of
/// `agent`, most valuable first.
///
/// A bundle's value is the best total gain over every order in which the agent can build it,
/// inserting each task where [`Valuation::best_insertion`] puts it. Bundles worth nothing are
/// left out. Of equally valuable bundles, the one with the smaller tasks comes first.
///
/// The search tries every build order of up to `max_size` tasks: about `tasks^max_size`
/// calls to the valuation. Keep `max_size` small when there are many tasks.
///
/// # Examples
///
/// ```
/// use task_allocation::{AgentId, TaskId, Valuation, auction::bundle_bids};
///
/// /// Tasks 0 and 1 are worth 3 alone and 10 together; task 2 is worth 4.
/// struct Pair;
///
/// impl Valuation<TaskId> for Pair {
///     fn marginal_score(&self, path: &[TaskId], task: &TaskId) -> Option<u64> {
///         let partner = TaskId(1 - task.index().min(1));
///         Some(match task.index() {
///             2 => 4,
///             _ if path.contains(&partner) => 7,
///             _ => 3,
///         })
///     }
/// }
///
/// let bids = bundle_bids(AgentId(0), &Pair, 3, 2, 2);
///
/// assert_eq!(bids[0].bundle.path(), [TaskId(0), TaskId(1)]);
/// assert_eq!(bids[0].value(), 10);
/// assert_eq!(bids[1].value(), 7);
/// ```
#[must_use]
pub fn bundle_bids<V>(
    agent: AgentId,
    valuation: &V,
    tasks: usize,
    max_size: usize,
    limit: usize,
) -> Vec<BundleBid>
where
    V: Valuation<TaskId> + ?Sized,
{
    let mut best: BTreeMap<Vec<TaskId>, Bundle<TaskId>> = BTreeMap::new();
    let mut bundle = Bundle::new();
    record_bundles(valuation, tasks, max_size, &mut bundle, &mut best);

    let mut bundles: Vec<(Vec<TaskId>, Bundle<TaskId>)> = best
        .into_iter()
        .filter(|(_, bundle)| bundle.value() > 0)
        .collect();
    bundles.sort_by(|(left_tasks, left), (right_tasks, right)| {
        right
            .value()
            .cmp(&left.value())
            .then_with(|| left_tasks.cmp(right_tasks))
    });

    bundles
        .into_iter()
        .take(limit)
        .map(|(_, bundle)| BundleBid { agent, bundle })
        .collect()
}

/// Records `bundle` as the best for its task set if it beats what is recorded, then tries
/// every task that extends it, up to `max_size` tasks.
fn record_bundles<V>(
    valuation: &V,
    tasks: usize,
    max_size: usize,
    bundle: &mut Bundle<TaskId>,
    best: &mut BTreeMap<Vec<TaskId>, Bundle<TaskId>>,
) where
    V: Valuation<TaskId> + ?Sized,
{
    let mut key: Vec<TaskId> = bundle.path().to_vec();
    key.sort_unstable();
    let beats_best = best
        .get(&key)
        .is_none_or(|current| bundle.value() > current.value());
    if beats_best {
        best.insert(key, bundle.clone());
    }

    if bundle.len() >= max_size {
        return;
    }

    for task in (0..tasks).map(TaskId) {
        if bundle.contains(&task) {
            continue;
        }

        let Some(insertion) = valuation.best_insertion(bundle.path(), &task) else {
            continue;
        };

        let index = bundle.len();
        bundle.insert(task, insertion);
        record_bundles(valuation, tasks, max_size, bundle, best);
        bundle.release_from(index);
    }
}

/// The winning bids: at most one per agent, no task in two of them, with the highest total
/// value; returned as the assignment of `agents` agents they make up.
///
/// Agents without a winning bid get an empty bundle. Bids of agents outside `0..agents` are
/// ignored. Of equally valuable selections, the first found wins, preferring each agent's
/// bids in the order given.
///
/// This is exact winner determination by depth-first branch and bound over agents, pruned
/// with each remaining agent's best bid. It is NP-hard in general: plan for at most a few
/// dozen bids.
///
/// # Examples
///
/// ```
/// use task_allocation::{AgentId, Bundle, Insertion, TaskId, auction::{BundleBid, winner_determination}};
///
/// /// A bid of `agent` on `tasks`, worth `value` in all.
/// fn bid(agent: usize, tasks: &[usize], value: u64) -> BundleBid {
///     let mut bundle = Bundle::new();
///     for (position, &task) in tasks.iter().enumerate() {
///         let gain = if position == 0 { value } else { 0 };
///         bundle.insert(TaskId(task), Insertion { position, gain });
///     }
///     BundleBid { agent: AgentId(agent), bundle }
/// }
///
/// let bids = [bid(0, &[0, 1], 10), bid(1, &[0], 6), bid(2, &[1], 6)];
/// let assignment = winner_determination(&bids, 3);
///
/// // Agents 1 and 2 together beat agent 0's pair.
/// assert_eq!(assignment.value(), 12);
/// assert_eq!(assignment.holder(TaskId(1)), Some(AgentId(2)));
/// ```
#[must_use]
pub fn winner_determination(bids: &[BundleBid], agents: usize) -> Assignment {
    let mut by_agent: Vec<Vec<&BundleBid>> = vec![Vec::new(); agents];
    for bid in bids.iter().filter(|bid| bid.agent.index() < agents) {
        by_agent[bid.agent.index()].push(bid);
    }

    let mut search = Search::new(by_agent);
    search.explore(0, 0);

    let bundles = search
        .best_choice
        .iter()
        .zip(&search.by_agent)
        .map(|(choice, bids)| choice.map_or_else(Bundle::new, |index| bids[index].bundle.clone()))
        .collect();

    Assignment::from_bundles(bundles)
}

/// The combinatorial auction of tasks `0..tasks` among `agents`: each agent bids on its
/// `bids_per_agent` most valuable bundles of at most `capacity` tasks (see [`bundle_bids`]),
/// and [`winner_determination`] picks the winners.
///
/// With enough bids per agent, this finds the optimal assignment; with few, it trades
/// quality for a smaller search.
///
/// # Examples
///
/// ```
/// use task_allocation::{TaskId, Valuation, auction::combinatorial};
///
/// /// Values each task at a fixed reward, for at most one task.
/// struct Single([u64; 2]);
///
/// impl Valuation<TaskId> for Single {
///     fn marginal_score(&self, path: &[TaskId], task: &TaskId) -> Option<u64> {
///         path.is_empty().then_some(self.0[task.index()])
///     }
/// }
///
/// let agents = [Single([5, 0]), Single([6, 5])];
///
/// assert_eq!(combinatorial(&agents, 2, 1, 2).value(), 10);
/// ```
#[must_use]
pub fn combinatorial<V>(
    agents: &[V],
    tasks: usize,
    capacity: usize,
    bids_per_agent: usize,
) -> Assignment
where
    V: Valuation<TaskId>,
{
    let bids: Vec<BundleBid> = agents
        .iter()
        .enumerate()
        .flat_map(|(index, valuation)| {
            bundle_bids(AgentId(index), valuation, tasks, capacity, bids_per_agent)
        })
        .collect();

    winner_determination(&bids, agents.len())
}

/// The state of the branch-and-bound search over one bid per agent.
struct Search<'a> {
    /// Each agent's bids.
    by_agent: Vec<Vec<&'a BundleBid>>,

    /// For each agent, the total of the best bid of it and every later agent: the most the
    /// remaining agents can add.
    bound_from: Vec<u64>,

    /// The tasks the current selection already uses.
    taken: BTreeSet<TaskId>,

    /// The bid index chosen for each agent so far; `None` for no bid.
    choice: Vec<Option<usize>>,

    /// The best selection found, by bid index per agent.
    best_choice: Vec<Option<usize>>,

    /// The value of the best selection; `None` before the first complete one.
    best_value: Option<u64>,
}

impl<'a> Search<'a> {
    /// A search over `by_agent`, with nothing chosen yet.
    fn new(by_agent: Vec<Vec<&'a BundleBid>>) -> Self {
        let agents = by_agent.len();
        let mut bound_from: Vec<u64> = vec![0; agents + 1];
        for agent in (0..agents).rev() {
            let best = by_agent[agent]
                .iter()
                .map(|bid| bid.value())
                .max()
                .unwrap_or(0);
            bound_from[agent] = bound_from[agent + 1].saturating_add(best);
        }

        Self {
            by_agent,
            bound_from,
            taken: BTreeSet::new(),
            choice: vec![None; agents],
            best_choice: vec![None; agents],
            best_value: None,
        }
    }

    /// Chooses a bid, or none, for `agent` and every later agent, given the earlier choices
    /// worth `value`.
    fn explore(&mut self, agent: usize, value: u64) {
        let hopeless = self
            .best_value
            .is_some_and(|best| value.saturating_add(self.bound_from[agent]) <= best);
        if hopeless {
            return;
        }

        if agent == self.by_agent.len() {
            self.best_value = Some(value);
            self.best_choice.clone_from(&self.choice);
            return;
        }

        for index in 0..self.by_agent[agent].len() {
            let bid = self.by_agent[agent][index];
            if bid.bundle.tasks().any(|task| self.taken.contains(task)) {
                continue;
            }

            self.mark(bid, true);
            self.choice[agent] = Some(index);
            self.explore(agent + 1, value.saturating_add(bid.value()));
            self.choice[agent] = None;
            self.mark(bid, false);
        }

        self.explore(agent + 1, value);
    }

    /// Marks the tasks of `bid` as taken or free.
    fn mark(&mut self, bid: &BundleBid, taken: bool) {
        for &task in bid.bundle.tasks() {
            if taken {
                self.taken.insert(task);
            } else {
                self.taken.remove(&task);
            }
        }
    }
}
