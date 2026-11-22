//! Two rovers split survey tasks with CBBA; then temporal-control checks a timing constraint
//! between them that no single rover's valuation can see: the uplink of the core sample must
//! start between 10 and 40 seconds after the other rover finishes drilling it.
//!
//! The rovers' schedules go into a simple temporal network, anchored at a shared origin, with
//! each visit allowed to slip until its deadline. The network then says whether both rovers
//! can keep their plans and still meet the handoff, and how late each visit may start. When
//! they cannot, the mission tightens the uplink's deadline to the end of the handoff window
//! and lets the rovers allocate again.
//!
//! Run with `cargo run --example relay_timing`.

use task_allocation::cbba::Agent;
use task_allocation::route::{Route, Timed, Visit, rewards};
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, BestInsertion, TaskId, check_diminishing};
use temporal_control::{Budget, Stn, Ticks, TimePoint, Verdict};

/// A survey site: where on the line, how long it takes, its deadline, and its reward.
struct Site {
    /// Position on the line, in meters.
    at: u64,
    /// Seconds of work once there.
    duration: u64,
    /// Latest finish, in seconds after the start.
    deadline: u64,
    /// Reward for finishing in time.
    reward: u64,
}

/// The sites. Task 1 drills the core sample, task 4 uplinks it.
const SITES: [Site; 5] = [
    Site {
        at: 10,
        duration: 5,
        deadline: 100,
        reward: 30,
    },
    Site {
        at: 25,
        duration: 15,
        deadline: 80,
        reward: 80,
    },
    Site {
        at: 50,
        duration: 5,
        deadline: 120,
        reward: 30,
    },
    Site {
        at: 70,
        duration: 10,
        deadline: 150,
        reward: 40,
    },
    Site {
        at: 95,
        duration: 5,
        deadline: 150,
        reward: 60,
    },
];

/// The drilling task.
const DRILL: TaskId = TaskId(1);

/// The uplink task, which needs the drilled sample's data.
const UPLINK: TaskId = TaskId(4);

/// A rover on the line, at one meter per second.
#[derive(Clone, Copy)]
struct Rover {
    /// Where the rover starts, in meters.
    start: u64,
    /// The uplink's deadline, which the mission may tighten below the site's.
    uplink_deadline: u64,
}

impl Rover {
    /// The latest finish of `task` for this rover.
    fn deadline(&self, task: TaskId) -> u64 {
        if task == UPLINK {
            self.uplink_deadline
        } else {
            SITES[task.index()].deadline
        }
    }
}

impl Route for Rover {
    type Place = u64;

    fn origin(&self) -> u64 {
        self.start
    }

    fn place(&self, task: TaskId) -> u64 {
        SITES[task.index()].at
    }

    fn travel(&self, from: &u64, to: &u64) -> Option<u64> {
        Some(from.abs_diff(*to))
    }

    fn duration(&self, task: TaskId) -> u64 {
        SITES[task.index()].duration
    }

    fn reward(&self, task: TaskId, finish: u64) -> Option<u64> {
        rewards::deadline(SITES[task.index()].reward, finish, self.deadline(task))
    }
}

/// Seconds as temporal-control ticks.
fn ticks(seconds: u64) -> Ticks {
    Ticks(i64::try_from(seconds).expect("mission times fit"))
}

/// Adds one rover's visits to `stn`: each start no earlier than planned and late enough to
/// finish by the deadline, in order, with the work and the drive between them. Returns the
/// start time point of each task.
fn add_rover(
    stn: &mut Stn,
    origin: TimePoint,
    rover: &Rover,
    visits: &[Visit],
) -> Vec<(TaskId, TimePoint)> {
    let mut starts = Vec::new();
    let mut previous: Option<(&Visit, TimePoint)> = None;

    for visit in visits {
        let site = &SITES[visit.task.index()];
        let start = stn.add_time_point().expect("fits");
        let latest = rover.deadline(visit.task) - site.duration;
        stn.add_constraint(origin, start, Some(ticks(visit.start)), Some(ticks(latest)))
            .expect("time points exist");

        if let Some((before, before_start)) = previous {
            let drive = rover
                .travel(&SITES[before.task.index()].at, &site.at)
                .expect("on the line");
            let gap = SITES[before.task.index()].duration + drive;
            stn.add_constraint(before_start, start, Some(ticks(gap)), None)
                .expect("time points exist");
        }

        starts.push((visit.task, start));
        previous = Some((visit, start));
    }

    starts
}

/// The outcome of one allocation round.
enum Check {
    /// The plans meet the handoff.
    Met,
    /// The plans cannot meet the handoff.
    Missed {
        /// When drilling ends in the plans.
        drilling_ends: u64,
    },
    /// Drilling or uplink went unassigned.
    Unassigned,
}

/// The rovers' plans as a simple temporal network.
struct Plans {
    /// The network.
    stn: Stn,
    /// The shared origin: time zero.
    origin: TimePoint,
    /// The start time point of every assigned task.
    starts: Vec<(TaskId, TimePoint)>,
    /// When drilling ends in the plans; zero when nobody drills.
    drilling_ends: u64,
}

/// Allocates the sites among `rovers` with CBBA over a slightly delayed network.
fn allocate(rovers: &[Rover]) -> Simulation<Agent<BestInsertion<Timed<Rover>>>> {
    // Fixed rewards with deadlines on a line keep the gains diminishing, which CBBA needs
    // to converge.
    let tasks: Vec<TaskId> = (0..SITES.len()).map(TaskId).collect();
    for rover in rovers {
        check_diminishing(&BestInsertion(Timed(*rover)), &tasks, 3).expect("gains diminish");
    }

    let agents = rovers
        .iter()
        .enumerate()
        .map(|(id, rover)| {
            Agent::new(
                AgentId(id),
                rovers.len(),
                SITES.len(),
                3,
                BestInsertion(Timed(*rover)),
            )
        })
        .collect();
    let mut simulation = Simulation::new(
        agents,
        Network {
            seed: 3,
            max_delay: 2,
            ..Network::default()
        },
    );
    simulation
        .run_until_settled(1_000)
        .expect("the rovers agree");

    simulation
}

/// Puts every rover's settled plan into one network, printing each plan.
fn plans(simulation: &Simulation<Agent<BestInsertion<Timed<Rover>>>>) -> Plans {
    let mut stn = Stn::new();
    let origin = stn.add_time_point().expect("fits");
    let mut starts = Vec::new();
    let mut drilling_ends = 0;

    for agent in simulation.agents() {
        let timed = &agent.valuation().0;
        let visits = timed.schedule(agent.bundle().path()).expect("feasible");
        println!(
            "  rover {}: {:?}",
            agent.id().index(),
            visits
                .iter()
                .map(|visit| (visit.task.index(), visit.start))
                .collect::<Vec<_>>()
        );

        if let Some(visit) = visits.iter().find(|visit| visit.task == DRILL) {
            drilling_ends = visit.finish;
        }

        starts.extend(add_rover(&mut stn, origin, &timed.0, &visits));
    }

    Plans {
        stn,
        origin,
        starts,
        drilling_ends,
    }
}

/// Adds the handoff to `plans` and checks the network, printing each task's start window
/// when it is consistent.
fn check_handoff(mut plans: Plans) -> Check {
    let start_of = |task: TaskId| {
        plans
            .starts
            .iter()
            .find(|(held, _)| *held == task)
            .map(|(_, point)| *point)
    };
    let (Some(drill), Some(uplink)) = (start_of(DRILL), start_of(UPLINK)) else {
        return Check::Unassigned;
    };

    // The uplink starts 10 to 40 seconds after drilling ends.
    let drilling = SITES[DRILL.index()].duration;
    plans
        .stn
        .add_constraint(
            drill,
            uplink,
            Some(ticks(drilling + 10)),
            Some(ticks(drilling + 40)),
        )
        .expect("time points exist");

    match plans
        .stn
        .minimal_network(&Budget::UNLIMITED)
        .expect("small network")
    {
        Verdict::Consistent(network) => {
            for (task, point) in &plans.starts {
                let earliest = network.lower(plans.origin, *point).expect("bounded");
                let latest = network.upper(plans.origin, *point).expect("bounded");
                println!(
                    "  task {} may start from {} to {} s",
                    task.index(),
                    earliest.0,
                    latest.0
                );
            }

            Check::Met
        }
        Verdict::Inconsistent(cycle) => {
            println!(
                "  the handoff cannot be met: {} constraints conflict",
                cycle.edges().len()
            );

            Check::Missed {
                drilling_ends: plans.drilling_ends,
            }
        }
    }
}

/// Allocates the sites among `rovers`, then checks the handoff.
fn allocate_and_check(rovers: &[Rover]) -> Check {
    check_handoff(plans(&allocate(rovers)))
}

fn main() {
    let uplink_deadline = SITES[UPLINK.index()].deadline;
    let mut rovers = [
        Rover {
            start: 0,
            uplink_deadline,
        },
        Rover {
            start: 100,
            uplink_deadline,
        },
    ];

    println!("first allocation:");
    let Check::Missed { drilling_ends } = allocate_and_check(&rovers) else {
        return;
    };

    // The uplink must now finish within the handoff window after drilling.
    let tightened = drilling_ends + 40 + SITES[UPLINK.index()].duration;
    for rover in &mut rovers {
        rover.uplink_deadline = tightened;
    }

    println!("with the uplink due by {tightened} s:");
    let outcome = allocate_and_check(&rovers);
    assert!(
        matches!(outcome, Check::Met),
        "the tightened deadline meets the handoff"
    );
}
