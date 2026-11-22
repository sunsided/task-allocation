//! Benchmarks on the colony workload of `examples/colony_sim.rs`: 50 workers, 200 jobs with
//! deadlines, up to four queued each.
//!
//! - `sequential_greedy`: one central pass.
//! - `cbba/settle`: a whole CBBA negotiation, from nothing to settled.
//! - `cbba/round`: one round in the middle of a negotiation, as a game would run per frame.
//!
//! The timings run on the system allocator. The allocations of a round are counted in
//! `tests/alloc`, which swaps in a counting allocator.
//!
//! Run with `task bench`, or `cargo bench --bench colony`.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group};
use task_allocation::centralized::sequential_greedy;

/// The colony workload, shared with the allocation test.
#[path = "support/colony.rs"]
mod colony;

use colony::{JOBS, QUEUE, WARM_UP_ROUNDS, colony, negotiation};

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

criterion::criterion_main!(colony_benches);
