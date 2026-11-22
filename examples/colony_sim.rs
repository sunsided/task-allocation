//! A colony sim: 50 workers and 200 jobs on a 100 × 100 map. Each job has a site, a duration,
//! a reward and a deadline; workers walk in Manhattan steps and hold up to four jobs each.
//!
//! The example allocates the jobs three ways and compares them: sequential greedy (one
//! central pass, the usual game-AI baseline), a sequential auction in deadline order, and
//! CBBA, where every worker bids from its own view and the colony agrees on bundles. Then a
//! job appears and a worker leaves, and the colony allocates again; the example counts how
//! many workers' plans change.
//!
//! The job set is fixed when agents are created, so reacting to a change means a new
//! allocation round. Fixed rewards with deadlines over a metric keep the gains diminishing,
//! which CBBA needs to converge.
//!
//! Run with `cargo run --release --example colony_sim`.

use std::time::Instant;

use task_allocation::auction::sequential;
use task_allocation::cbba::{Agent, Replan};
use task_allocation::centralized::sequential_greedy;
use task_allocation::route::{Route, Timed, rewards};
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, Assignment, BestInsertion, TaskId};

/// Workers in the colony.
const WORKERS: usize = 50;

/// Jobs on the board.
const JOBS: usize = 200;

/// The most jobs one worker queues up.
const QUEUE: usize = 4;

/// A cell of the map, as column and row.
type Cell = (u64, u64);

/// A job: where it is, how long it takes, what it pays, and by when it must be done.
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

/// A worker: where it stands, and the job board it reads.
#[derive(Clone, Debug)]
struct Worker {
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

    /// One tick per step, four-connected.
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

/// A small seeded generator, so every run of the example builds the same colony.
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

    /// A random job.
    fn job(&mut self) -> Job {
        Job {
            site: self.cell(),
            duration: self.roll(2, 12),
            reward: self.roll(10, 100),
            deadline: self.roll(40, 200),
        }
    }
}

/// The workers' valuation: their walk, with each job slotted in where it adds the most.
type Plan = BestInsertion<Timed<Worker>>;

/// Every worker's valuation, for `homes` and the job board `jobs`.
fn workers(homes: &[Cell], jobs: &[Job]) -> Vec<Plan> {
    homes
        .iter()
        .map(|&home| {
            BestInsertion(Timed(Worker {
                home,
                jobs: jobs.to_vec(),
            }))
        })
        .collect()
}

/// Allocates `tasks` jobs among `plans` with CBBA, every worker replanning as `replan` says;
/// returns the settled assignment and the number of rounds it took.
fn cbba(plans: &[Plan], tasks: usize, replan: Replan) -> (Assignment, u64) {
    let team = plans.len();
    let agents = plans
        .iter()
        .cloned()
        .enumerate()
        .map(|(id, plan)| Agent::new(AgentId(id), team, tasks, QUEUE, plan).with_replan(replan))
        .collect();
    let mut simulation = Simulation::new(agents, Network::default());
    let rounds = simulation
        .run_until_settled(10_000)
        .expect("the colony agrees");

    (simulation.assignment(), rounds)
}

/// Prints one line per allocation: its name, value, jobs taken and time.
fn report(name: &str, assignment: &Assignment, started: Instant, note: &str) {
    let taken: usize = assignment
        .bundles()
        .iter()
        .map(task_allocation::Bundle::len)
        .sum();
    println!(
        "{name:>20}: value {:>5}, {taken:>3} jobs, {:>7.1} ms{note}",
        assignment.value(),
        started.elapsed().as_secs_f64() * 1000.0,
    );
}

/// How many workers' plans differ between `before` and `after`, comparing the first
/// `workers` workers' queues as sets of jobs.
fn changed_plans(before: &Assignment, after: &Assignment, workers: usize) -> usize {
    (0..workers)
        .filter(|&worker| {
            let mut old: Vec<TaskId> = before.bundle(AgentId(worker)).tasks().copied().collect();
            let mut new: Vec<TaskId> = after.bundle(AgentId(worker)).tasks().copied().collect();
            old.sort_unstable();
            new.sort_unstable();
            old != new
        })
        .count()
}

/// Allocates the board with every method and prints how they compare; returns CBBA's
/// assignment, the reference for the changes that follow.
fn compare(plans: &[Plan], jobs: &[Job]) -> Assignment {
    println!("{WORKERS} workers, {JOBS} jobs, up to {QUEUE} queued each:");

    let started = Instant::now();
    let greedy = sequential_greedy(plans, JOBS, QUEUE);
    report("sequential greedy", &greedy, started, "");

    let mut by_deadline: Vec<TaskId> = (0..JOBS).map(TaskId).collect();
    by_deadline.sort_by_key(|task| jobs[task.index()].deadline);
    let started = Instant::now();
    let auction = sequential(plans, JOBS, QUEUE, by_deadline);
    report("auction by deadline", &auction, started, "");

    let started = Instant::now();
    let (baseline, rounds) = cbba(plans, JOBS, Replan::Keep);
    report(
        "CBBA",
        &baseline,
        started,
        &format!(", settled in {rounds} rounds"),
    );
    assert!(baseline.is_conflict_free());

    let started = Instant::now();
    let (full, rounds) = cbba(plans, JOBS, Replan::Full);
    report(
        "CBBA, full replan",
        &full,
        started,
        &format!(", settled in {rounds} rounds"),
    );
    assert!(full.is_conflict_free());

    baseline
}

/// Adds a rush job next to worker 0, allocates again, and prints who takes it and how many
/// plans change.
fn rush_job(homes: &[Cell], jobs: &[Job], baseline: &Assignment) {
    println!("a rush job appears next to worker 0:");
    let (x, y) = homes[0];
    let mut jobs = jobs.to_vec();
    jobs.push(Job {
        site: (x.saturating_sub(1), y),
        duration: 3,
        reward: 150,
        deadline: 30,
    });

    let started = Instant::now();
    let (with_rush, rounds) = cbba(&workers(homes, &jobs), JOBS + 1, Replan::Keep);
    report(
        "CBBA",
        &with_rush,
        started,
        &format!(", settled in {rounds} rounds"),
    );
    println!(
        "{:>20}  rush job taken by worker {:?}; {} of {WORKERS} plans changed",
        "",
        with_rush.holder(TaskId(JOBS)).map(|worker| worker.0),
        changed_plans(baseline, &with_rush, WORKERS),
    );
}

/// Removes worker 0, allocates again, and prints how many of its jobs found a new worker.
fn worker_leaves(homes: &[Cell], jobs: &[Job], baseline: &Assignment) {
    println!("worker 0 leaves:");
    let started = Instant::now();
    let (without, rounds) = cbba(&workers(&homes[1..], jobs), JOBS, Replan::Keep);
    report(
        "CBBA",
        &without,
        started,
        &format!(", settled in {rounds} rounds"),
    );

    let orphans = baseline.bundle(AgentId(0));
    let rehomed = orphans
        .tasks()
        .filter(|&&task| without.holder(task).is_some())
        .count();
    println!(
        "{:>20}  {rehomed} of worker 0's {} jobs found a new worker",
        "",
        orphans.len()
    );
}

fn main() {
    let mut dice = Dice(2026);
    let homes: Vec<Cell> = (0..WORKERS).map(|_| dice.cell()).collect();
    let jobs: Vec<Job> = (0..JOBS).map(|_| dice.job()).collect();

    let baseline = compare(&workers(&homes, &jobs), &jobs);
    rush_job(&homes, &jobs, &baseline);
    worker_leaves(&homes, &jobs, &baseline);
}
