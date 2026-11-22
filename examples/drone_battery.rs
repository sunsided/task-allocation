//! Three survey drones split inspection points with CBBA. Each drone checks its own battery
//! inside its valuation with resource-timeline: flying drains one unit per second, every
//! photo costs a fixed amount, and a plan that would dip into the 20% reserve is infeasible.
//! After allocation, a shared ground-station link is checked across all drones: it carries
//! one upload at a time, which the drones' plans do not know about.
//!
//! Run with `cargo run --example drone_battery`.

use resource_timeline::{ActivityId, Bounds, Impact, Timeline};
use task_allocation::cbba::Agent;
use task_allocation::route::{Route, Timed, Visit, rewards};
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, BestInsertion, PathScore, TaskId};

/// Inspection points on a 100 × 100 meter field: position and photo energy.
const POINTS: [((u64, u64), i64); 9] = [
    ((10, 80), 10),
    ((30, 20), 5),
    ((50, 50), 15),
    ((70, 90), 10),
    ((90, 10), 5),
    ((20, 50), 10),
    ((80, 40), 15),
    ((60, 70), 5),
    ((40, 90), 10),
];

/// Seconds a photo and its upload take.
const PHOTO_SECONDS: u64 = 4;

/// A drone that flies at one meter per second in Manhattan directions.
#[derive(Clone, Copy)]
struct Flight {
    /// Where the drone takes off.
    base: (u64, u64),
}

impl Route for Flight {
    type Place = (u64, u64);

    fn origin(&self) -> (u64, u64) {
        self.base
    }

    fn place(&self, task: TaskId) -> (u64, u64) {
        POINTS[task.index()].0
    }

    fn travel(&self, from: &(u64, u64), to: &(u64, u64)) -> Option<u64> {
        Some(from.0.abs_diff(to.0) + from.1.abs_diff(to.1))
    }

    fn duration(&self, _task: TaskId) -> u64 {
        PHOTO_SECONDS
    }

    /// Photos lose value as the light changes: worth 100, halving every two minutes.
    fn reward(&self, _task: TaskId, finish: u64) -> Option<u64> {
        Some(rewards::halving(100, finish, 120))
    }
}

/// A drone's flight plan, made infeasible by its battery as well as by its route.
struct Drone {
    /// Where it flies.
    flight: Timed<Flight>,
    /// Battery units at take-off.
    charge: i64,
}

/// A timeline tick for a schedule time in seconds.
fn tick(seconds: u64) -> i64 {
    i64::try_from(seconds).expect("mission times fit")
}

impl Drone {
    /// The battery's level over `visits`: draining while flying, stepping down per photo,
    /// never below a 20% reserve.
    fn battery(&self, visits: &[Visit]) -> Result<Timeline, resource_timeline::Error> {
        let reserve = self.charge / 5;
        let mut battery = Timeline::depletable(
            0,
            self.charge,
            Bounds::new(reserve, self.charge)?,
            2 * visits.len() + 1,
        )?;
        let mut leaves = 0;

        for (index, visit) in visits.iter().enumerate() {
            let flight = ActivityId(u32::try_from(2 * index).expect("few visits"));
            if visit.arrival > leaves {
                battery.add(
                    flight,
                    Impact::Rate {
                        start: tick(leaves),
                        end: tick(visit.arrival),
                        per_tick: -1,
                    },
                )?;
            }

            let photo = ActivityId(u32::try_from(2 * index + 1).expect("few visits"));
            battery.add(
                photo,
                Impact::Step {
                    at: tick(visit.start),
                    delta: -POINTS[visit.task.index()].1,
                },
            )?;
            leaves = visit.finish;
        }

        Ok(battery)
    }
}

impl PathScore<TaskId> for Drone {
    fn path_score(&self, path: &[TaskId]) -> Option<u64> {
        let visits = self.flight.schedule(path)?;
        let battery = self.battery(&visits).ok()?;
        let within_reserve = battery.first_violation().ok()?.is_none();

        within_reserve.then(|| visits.iter().map(|visit| visit.reward).sum())
    }
}

/// Checks the shared link: every photo uploads while it is taken, and the link carries one
/// upload at a time. Prints the first overload, if any.
fn check_link(
    simulation: &Simulation<BestInsertion<Drone>>,
) -> Result<(), resource_timeline::Error> {
    let mut link = Timeline::claimable(0, 1, POINTS.len())?;
    for agent in simulation.agents() {
        let drone = &agent.valuation().0;
        for visit in drone
            .flight
            .schedule(agent.bundle().path())
            .expect("feasible")
        {
            let activity = ActivityId(u32::try_from(visit.task.index()).expect("few points"));
            link.add(
                activity,
                Impact::Claim {
                    start: tick(visit.start),
                    end: tick(visit.finish),
                    amount: 1,
                },
            )?;
        }
    }

    match link.first_violation()? {
        None => println!("the link never carries two uploads at once"),
        Some(overload) => println!(
            "the link is overloaded at {} s by {:?}",
            overload.at, overload.contributors
        ),
    }

    Ok(())
}

fn main() -> Result<(), resource_timeline::Error> {
    let bases = [((0, 0), 300), ((100, 100), 300), ((0, 100), 150)];
    let agents = bases
        .iter()
        .enumerate()
        .map(|(id, &(base, charge))| {
            let drone = Drone {
                flight: Timed(Flight { base }),
                charge,
            };
            Agent::new(
                AgentId(id),
                bases.len(),
                POINTS.len(),
                4,
                BestInsertion(drone),
            )
        })
        .collect();

    let network = Network {
        seed: 9,
        max_delay: 2,
        loss_per_mille: 100,
        ..Network::default()
    };
    let mut simulation = Simulation::new(agents, network);
    simulation
        .run_until_settled(1_000)
        .expect("the drones agree");
    assert!(simulation.assignment().is_conflict_free());

    for agent in simulation.agents() {
        let drone = &agent.valuation().0;
        let visits = drone
            .flight
            .schedule(agent.bundle().path())
            .expect("feasible");
        let battery = drone.battery(&visits)?;
        let landing = visits.last().map_or(0, |visit| visit.finish);
        println!(
            "drone {}: points {:?}, battery {} at take-off, {} after the last photo",
            agent.id().index(),
            visits
                .iter()
                .map(|visit| visit.task.index())
                .collect::<Vec<_>>(),
            drone.charge,
            battery.level_at(tick(landing))?,
        );
    }

    let unassigned: Vec<usize> = (0..POINTS.len())
        .filter(|&task| simulation.assignment().holder(TaskId(task)).is_none())
        .collect();
    println!("unassigned points: {unassigned:?}");

    check_link(&simulation)
}
