//! The colony workload of `examples/colony_sim.rs`, shared by the colony benchmark and the
//! allocation test: 50 workers, 200 jobs with deadlines, up to four queued each, seed 2026.

use task_allocation::cbba::Agent;
use task_allocation::route::{Route, Timed, rewards};
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, BestInsertion, TaskId};

/// Workers in the colony.
pub const WORKERS: usize = 50;

/// Jobs on the board.
pub const JOBS: usize = 200;

/// The most jobs one worker queues up.
pub const QUEUE: usize = 4;

/// Rounds a negotiation runs before a single round is measured.
pub const WARM_UP_ROUNDS: usize = 10;

/// A cell of the map, as column and row.
type Cell = (u64, u64);

/// A job: where, how long, what it pays, and by when.
#[derive(Clone, Copy, Debug)]
struct Job {
    /// Where the job is done.
    site: Cell,
    /// Ticks of work.
    duration: u64,
    /// What finishing it in time pays.
    reward: u64,
    /// The tick by which it must be done.
    deadline: u64,
}

/// A worker walking in Manhattan steps.
#[derive(Clone, Debug)]
pub struct Worker {
    /// Where the worker starts.
    home: Cell,
    /// Every job, indexed by task.
    jobs: Vec<Job>,
}

impl Route for Worker {
    type Place = Cell;

    fn origin(&self) -> Cell {
        self.home
    }

    fn place(&self, task: TaskId) -> Cell {
        self.jobs[task.index()].site
    }

    fn travel(&self, from: &Cell, to: &Cell) -> Option<u64> {
        Some(from.0.abs_diff(to.0) + from.1.abs_diff(to.1))
    }

    fn duration(&self, task: TaskId) -> u64 {
        self.jobs[task.index()].duration
    }

    fn reward(&self, task: TaskId, finish: u64) -> Option<u64> {
        let job = self.jobs[task.index()];

        rewards::deadline(job.reward, finish, job.deadline)
    }
}

/// The same seeded generator as the example, so both build the same colony.
struct Dice(u64);

impl Dice {
    /// A number in `low..high`.
    fn roll(&mut self, low: u64, high: u64) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);

        low + (z ^ (z >> 31)) % (high - low)
    }

    /// A random cell of the map.
    fn cell(&mut self) -> Cell {
        (self.roll(0, 100), self.roll(0, 100))
    }
}

/// The workers' valuation.
pub type Plan = BestInsertion<Timed<Worker>>;

/// Every worker's valuation for the colony of seed 2026.
pub fn colony() -> Vec<Plan> {
    let mut dice = Dice(2026);
    let homes: Vec<Cell> = (0..WORKERS).map(|_| dice.cell()).collect();
    let jobs: Vec<Job> = (0..JOBS)
        .map(|_| Job {
            site: dice.cell(),
            duration: dice.roll(2, 12),
            reward: dice.roll(10, 100),
            deadline: dice.roll(40, 200),
        })
        .collect();

    homes
        .into_iter()
        .map(|home| {
            BestInsertion(Timed(Worker {
                home,
                jobs: jobs.clone(),
            }))
        })
        .collect()
}

/// A CBBA team for `plans` on a perfect network, at round zero.
pub fn negotiation(plans: &[Plan]) -> Simulation<Agent<Plan>> {
    let agents = plans
        .iter()
        .cloned()
        .enumerate()
        .map(|(id, plan)| Agent::new(AgentId(id), WORKERS, JOBS, QUEUE, plan))
        .collect();

    Simulation::new(agents, Network::default())
}
