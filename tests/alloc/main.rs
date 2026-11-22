//! Allocation check: how much one CBBA round allocates on the colony workload.
//!
//! This is its own test binary because it replaces the global allocator with a counting one,
//! and it holds a single test so no other test allocates while a region is measured. The
//! budgets sit about 10% above today's counts; each improvement should lower them.

use std::alloc::System;

use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use task_allocation::AgentId;
use task_allocation::cbba::Agent;
use task_allocation::sim::Simulation;

/// The colony workload, shared with the colony benchmark.
#[path = "../../benches/support/colony.rs"]
mod colony;

use colony::{JOBS, Plan, QUEUE, WARM_UP_ROUNDS, WORKERS, colony, negotiation};

/// Counts every allocation, so the test can measure a region.
#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

/// The most allocations a round mid negotiation may make.
const BUSY_BUDGET: usize = 290;

/// The most allocations a round of a settled team may make.
const SETTLED_BUDGET: usize = 225;

/// The allocations and bytes of one round of `simulation`.
fn allocations_of_one_round(simulation: &mut Simulation<Agent<Plan>>) -> (usize, usize) {
    let region = Region::new(GLOBAL);
    simulation.step();
    let stats = region.change();

    (stats.allocations, stats.bytes_allocated)
}

/// One round stays within its allocation budget, mid negotiation and once settled, and an
/// agent's own bundle phase and consensus phase allocate nothing; only building the message
/// it broadcasts does.
///
/// A single test, so no other test allocates while a region is measured.
#[test]
fn rounds_stay_within_their_allocation_budget() {
    let mut simulation = negotiation(&colony());
    for _ in 0..WARM_UP_ROUNDS {
        simulation.step();
    }

    let (busy, busy_bytes) = allocations_of_one_round(&mut simulation);
    simulation.run_until_settled(10_000).expect("settles");
    let (settled, settled_bytes) = allocations_of_one_round(&mut simulation);

    println!(
        "cbba round, {WORKERS} x {JOBS}: {busy} allocations ({busy_bytes} bytes) mid negotiation, {settled} ({settled_bytes} bytes) once settled"
    );
    assert!(busy <= BUSY_BUDGET, "{busy} allocations mid negotiation");
    assert!(
        settled <= SETTLED_BUDGET,
        "{settled} allocations once settled"
    );

    agent_phases_do_not_allocate(&colony());
}

/// Drives two colony agents directly, as a game would: after a warm-up, a bundle phase and
/// a consensus phase allocate nothing, and a message allocates its two tables.
fn agent_phases_do_not_allocate(plans: &[Plan]) {
    let mut agents: Vec<Agent<Plan>> = plans
        .iter()
        .take(2)
        .cloned()
        .enumerate()
        .map(|(id, plan)| Agent::new(AgentId(id), 2, JOBS, QUEUE, plan))
        .collect();
    for _ in 0..WARM_UP_ROUNDS {
        exchange(&mut agents);
    }

    let message = agents[1].message();
    let region = Region::new(GLOBAL);
    agents[0].build_bundle();
    let building = region.change().allocations;
    let region = Region::new(GLOBAL);
    agents[0].receive(&message);
    let receiving = region.change().allocations;
    let phases = building + receiving;

    let region = Region::new(GLOBAL);
    let broadcast = agents[0].message();
    let messaging = region.change().allocations;
    drop(broadcast);

    assert_eq!(phases, 0, "bundle and consensus phases allocated");
    assert_eq!(
        messaging, 2,
        "a message allocates its claim and clock tables"
    );
}

/// One round between `agents`: both build their bundles and read each other's message.
fn exchange(agents: &mut [Agent<Plan>]) {
    for agent in agents.iter_mut() {
        agent.build_bundle();
    }

    let messages: Vec<_> = agents.iter_mut().map(Agent::message).collect();
    agents[0].receive(&messages[1]);
    agents[1].receive(&messages[0]);
}
