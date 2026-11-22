# task-allocation

[![crates.io](https://img.shields.io/crates/v/task-allocation.svg)](https://crates.io/crates/task-allocation)
[![docs.rs](https://img.shields.io/docsrs/task-allocation)](https://docs.rs/task-allocation)
[![CI](https://github.com/sunsided/task-allocation/actions/workflows/ci.yml/badge.svg)](https://github.com/sunsided/task-allocation/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/sunsided/task-allocation/graph/badge.svg)](https://codecov.io/gh/sunsided/task-allocation)
[![MSRV](https://img.shields.io/crates/msrv/task-allocation)](https://crates.io/crates/task-allocation)
[![License](https://img.shields.io/crates/l/task-allocation)](https://github.com/sunsided/task-allocation#license)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)

Decentralized multi-agent task allocation in Rust: agents decide among themselves who does which task, exchanging bids whenever they can communicate, without a central authority.

## Scope

- Consensus-based bundle algorithm (CBBA) and its variants, converging to conflict-free assignments under delayed and intermittent communication
- Sequential and combinatorial auctions as baselines
- Task scores from user-supplied costs, such as travel time, deadlines or resource use
- A deterministic simulated network (seeded delays, losses and partitions) to test teams on
- Greedy bundle construction from marginal gains, with a check for diminishing gains
- Centralized references: the sequential greedy assignment, and the exhaustive optimum for small instances

Typical users are robot and rover teams, drone swarms and multi-agent simulations. Timing constraints between agents fit [temporal-control](https://github.com/sunsided/temporal-control), shared resource budgets [resource-timeline](https://github.com/sunsided/resource-timeline), and travel costs can come from [heuristic-search](https://github.com/sunsided/heuristic-search) or [sampling-planning](https://github.com/sunsided/sampling-planning).

## Usage

```sh
cargo add task-allocation
```

A valuation scores what a task is worth to one agent, given the tasks it already holds.
Agents bid with these scores, so a task that does not fit, such as one that would miss its
deadline, scores `None` and gets no bid. Here, a rover works through its bundle one task
after another.

```rust
use task_allocation::Valuation;

/// A task that takes `duration` minutes and must be done `deadline` minutes after the start.
struct Task {
    duration: u64,
    deadline: u64,
    reward: u64,
}

/// A rover that works through its bundle in order.
struct Rover;

impl Valuation<Task> for Rover {
    fn marginal_score(&self, bundle: &[Task], task: &Task) -> Option<u64> {
        let busy_until: u64 = bundle.iter().map(|held| held.duration).sum();

        // Appended last, the task starts once the rover has finished its bundle.
        (busy_until + task.duration <= task.deadline).then_some(task.reward)
    }
}

let survey = Task { duration: 30, deadline: 60, reward: 5 };
let sample = Task { duration: 20, deadline: 40, reward: 8 };

assert_eq!(Rover.marginal_score(&[], &sample), Some(8));

// After the survey, the sample would be done at minute 50, past its deadline.
assert_eq!(Rover.marginal_score(&[survey], &sample), None);
```

Allocation algorithms work on task identifiers. Each agent's valuation looks up what it
needs, and an algorithm returns one bundle per agent. Here, two rovers on a line split three
sites, each going for what is closest to its last stop:

```rust
use task_allocation::{AgentId, TaskId, Valuation, centralized::sequential_greedy};

/// Site positions along the line, in meters.
const SITES: [u64; 3] = [10, 40, 90];

/// A rover that earns 100 per site, minus the distance from its last stop.
struct Rover {
    start: u64,
}

impl Valuation<TaskId> for Rover {
    fn marginal_score(&self, path: &[TaskId], task: &TaskId) -> Option<u64> {
        let from = path.last().map_or(self.start, |last| SITES[last.index()]);

        100_u64.checked_sub(from.abs_diff(SITES[task.index()]))
    }
}

let rovers = [Rover { start: 0 }, Rover { start: 100 }];
let assignment = sequential_greedy(&rovers, SITES.len(), 2);

assert!(assignment.is_conflict_free());
assert_eq!(assignment.holder(TaskId(1)), Some(AgentId(0)));
assert_eq!(assignment.holder(TaskId(2)), Some(AgentId(1)));
```

Decentralized, each rover runs a CBBA agent and exchanges messages with whoever it can
reach. The simulated network here loses a third of all messages and delays the rest by up to
four ticks; the rovers still agree on who does what:

```rust
use task_allocation::cbba::Agent;
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, TaskId, Valuation};

/// The same rovers as above.
const SITES: [u64; 3] = [10, 40, 90];

struct Rover {
    start: u64,
}

impl Valuation<TaskId> for Rover {
    fn marginal_score(&self, path: &[TaskId], task: &TaskId) -> Option<u64> {
        let from = path.last().map_or(self.start, |last| SITES[last.index()]);

        100_u64.checked_sub(from.abs_diff(SITES[task.index()]))
    }
}

let agents = vec![
    Agent::new(AgentId(0), 2, SITES.len(), 2, Rover { start: 0 }),
    Agent::new(AgentId(1), 2, SITES.len(), 2, Rover { start: 100 }),
];
let network = Network { seed: 1, max_delay: 4, loss_per_mille: 333, ..Network::default() };
let mut simulation = Simulation::new(agents, network);

simulation.run_until_settled(1_000).expect("the rovers agree");

let assignment = simulation.assignment();
assert!(assignment.is_conflict_free());
assert_eq!(assignment.holder(TaskId(2)), Some(AgentId(1)));
```

On real hardware, call `build_bundle` and `message` on every round and pass each message
that arrives to `receive`.

The crate is `no_std`, needs `alloc`, and has no runtime dependencies.

## Minimum supported Rust version

Rust 1.85 (edition 2024). The MSRV applies to the library; tests may need a newer
toolchain. It may be raised in a minor release when a dependency or a language feature
calls for it.

## Contributing

See [CONTRIBUTING.md](https://github.com/sunsided/task-allocation/blob/main/CONTRIBUTING.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/sunsided/task-allocation/blob/main/LICENSE-APACHE))
- MIT license ([LICENSE-MIT](https://github.com/sunsided/task-allocation/blob/main/LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion
in this work by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
