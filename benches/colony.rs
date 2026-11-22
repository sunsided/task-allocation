//! Benchmarks on the colony workload of `examples/colony_sim.rs`: 50 workers, 200 jobs with
//! deadlines, up to four queued each.
//!
//! - `sequential_greedy`: one central pass.
//! - `cbba/settle`: a whole CBBA negotiation, from nothing to settled.
//! - `cbba/round`: one round in the middle of a negotiation, as a game would run per frame.
//!
//! Before the benchmarks, the allocations of one CBBA round are counted and printed, mid
//! negotiation and once settled; issue #17 tracks bringing them down.
//!
//! Run with `task bench`, or `cargo bench --bench colony`.

use std::alloc::System;
use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use task_allocation::cbba::Agent;
use task_allocation::centralized::sequential_greedy;
use task_allocation::route::{Route, Timed, rewards};
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, BestInsertion, TaskId};

/// Counts every allocation, for the per-round report.
#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

/// Workers in the colony.
const WORKERS: usize = 50;

/// Jobs on the board.
const JOBS: usize = 200;

/// The most jobs one worker queues up.
const QUEUE: usize = 4;

/// Rounds a negotiation runs before a single round is measured.
const WARM_UP_ROUNDS: usize = 10;

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
type Plan = BestInsertion<Timed<Worker>>;

/// Every worker's valuation for the colony of seed 2026.
fn colony() -> Vec<Plan> {
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
fn negotiation(plans: &[Plan]) -> Simulation<Agent<Plan>> {
    let agents = plans
        .iter()
        .cloned()
        .enumerate()
        .map(|(id, plan)| Agent::new(AgentId(id), WORKERS, JOBS, QUEUE, plan))
        .collect();

    Simulation::new(agents, Network::default())
}

/// The allocations and bytes of one round of `simulation`.
fn allocations_of_one_round(simulation: &mut Simulation<Agent<Plan>>) -> (usize, usize) {
    let region = Region::new(GLOBAL);
    simulation.step();
    let stats = region.change();

    (stats.allocations, stats.bytes_allocated)
}

/// Prints the allocations of one round, mid negotiation and once settled.
fn report_allocations(plans: &[Plan]) {
    let mut simulation = negotiation(plans);
    for _ in 0..WARM_UP_ROUNDS {
        simulation.step();
    }

    let (busy, busy_bytes) = allocations_of_one_round(&mut simulation);
    simulation.run_until_settled(10_000).expect("settles");
    let (settled, settled_bytes) = allocations_of_one_round(&mut simulation);

    eprintln!(
        "cbba round, {WORKERS} x {JOBS}: {busy} allocations ({busy_bytes} bytes) mid negotiation, {settled} ({settled_bytes} bytes) once settled"
    );
}

/// The benchmarks.
fn benchmarks(c: &mut Criterion) {
    let plans = colony();

    c.bench_function("sequential_greedy", |b| {
        b.iter(|| sequential_greedy(black_box(&plans), JOBS, QUEUE));
    });

    c.bench_function("cbba/settle", |b| {
        b.iter_batched(
            || negotiation(&plans),
            |mut simulation| simulation.run_until_settled(10_000),
            BatchSize::LargeInput,
        );
    });

    c.bench_function("cbba/round", |b| {
        b.iter_batched(
            || {
                let mut simulation = negotiation(&plans);
                for _ in 0..WARM_UP_ROUNDS {
                    simulation.step();
                }
                simulation
            },
            |mut simulation| {
                simulation.step();
                simulation
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group! {
    name = colony_benches;
    config = Criterion::default().sample_size(20);
    targets = benchmarks
}

fn main() {
    report_allocations(&colony());
    colony_benches();
    Criterion::default().configure_from_args().final_summary();
}
