//! Three robots in a warehouse split the pick-ups among themselves with CBBA, over a lossy
//! network. Travel times come from A* on the floor plan (heuristic-search), and every pick-up
//! has its own value and deadline. Exact shortest paths and fixed rewards keep the gains
//! diminishing, which CBBA needs to converge; the example checks that first.
//!
//! Run with `cargo run --example warehouse_grid`.

use std::collections::HashMap;

use heuristic_search::{Limits, Successors, a_star, heuristic_fn};
use task_allocation::cbba::Agent;
use task_allocation::route::{Route, Timed, rewards};
use task_allocation::sim::{Network, Simulation};
use task_allocation::{AgentId, BestInsertion, TaskId, check_diminishing};

/// The floor plan: `#` is a shelf, `A` to `C` are the robots' docks, `0` to `7` pick-ups.
const FLOOR: [&str; 9] = [
    "A..........#....B",
    ".####.####.#.###.",
    ".#0.#.#1.#...#2#.",
    ".#..#.#..#.#.#.#.",
    ".......#...#.....",
    ".####.##.#####.#.",
    ".#3..........4.#.",
    ".#####.###.#####.",
    "5.....C...6....7.",
];

/// The value of each pick-up, and the second by which it must be done.
const ORDERS: [(u64, u64); 8] = [
    (40, 30),
    (70, 40),
    (50, 25),
    (90, 45),
    (60, 35),
    (30, 20),
    (80, 30),
    (55, 50),
];

/// A cell of the floor plan, as column and row.
type Cell = (usize, usize);

/// The floor plan as a graph for A*: four neighbors per cell, one second per step.
struct Floor;

impl Floor {
    /// Whether a robot can drive over `cell`.
    fn is_free((x, y): Cell) -> bool {
        FLOOR
            .get(y)
            .and_then(|row| row.as_bytes().get(x))
            .is_some_and(|&byte| byte != b'#')
    }

    /// Where `label` is on the floor plan.
    fn find(label: u8) -> Cell {
        FLOOR
            .iter()
            .enumerate()
            .find_map(|(y, row)| row.bytes().position(|byte| byte == label).map(|x| (x, y)))
            .expect("every label is on the floor plan")
    }
}

impl Successors for Floor {
    type State = Cell;
    type Action = ();
    type Cost = u64;

    fn successors(&self, &(x, y): &Cell, mut emit: impl FnMut((), Cell, u64)) {
        let neighbors = [
            x.checked_sub(1).map(|left| (left, y)),
            Some((x + 1, y)),
            y.checked_sub(1).map(|up| (x, up)),
            Some((x, y + 1)),
        ];

        for cell in neighbors
            .into_iter()
            .flatten()
            .filter(|&cell| Self::is_free(cell))
        {
            emit((), cell, 1);
        }
    }
}

/// The shortest driving time between two cells, by A* with the Manhattan distance.
fn driving_time(from: Cell, to: Cell) -> Option<u64> {
    let manhattan =
        heuristic_fn(|&(x, y): &Cell| Some((x.abs_diff(to.0) + y.abs_diff(to.1)) as u64));
    let result = a_star(
        &Floor,
        from,
        &|cell: &Cell| *cell == to,
        manhattan,
        Limits::NONE,
    );

    result.solution().map(|solution| solution.cost)
}

/// One robot's view of the warehouse: its dock, and every driving time it may need.
struct Robot {
    /// Where the robot starts.
    dock: Cell,
    /// Every pick-up's cell, indexed by task.
    sites: Vec<Cell>,
    /// Driving times between every pair of relevant cells, computed once.
    times: HashMap<(Cell, Cell), u64>,
}

/// Every pick-up's cell, indexed by task.
fn pickups() -> Vec<Cell> {
    (b'0'..=b'7').map(Floor::find).collect()
}

impl Robot {
    /// A robot at `dock`, with driving times from the dock and between pick-ups.
    fn at(dock: Cell) -> Self {
        let sites = pickups();
        let cells: Vec<Cell> = std::iter::once(dock).chain(sites.iter().copied()).collect();
        let times = cells
            .iter()
            .flat_map(|&from| cells.iter().map(move |&to| (from, to)))
            .filter_map(|(from, to)| Some(((from, to), driving_time(from, to)?)))
            .collect();

        Self { dock, sites, times }
    }
}

impl Route for Robot {
    type Place = Cell;

    fn origin(&self) -> Cell {
        self.dock
    }

    fn place(&self, task: TaskId) -> Cell {
        self.sites[task.index()]
    }

    fn travel(&self, from: &Cell, to: &Cell) -> Option<u64> {
        self.times.get(&(*from, *to)).copied()
    }

    fn duration(&self, _task: TaskId) -> u64 {
        3
    }

    fn reward(&self, task: TaskId, finish: u64) -> Option<u64> {
        let (value, deadline) = ORDERS[task.index()];

        rewards::deadline(value, finish, deadline)
    }
}

fn main() {
    let docks = b"ABC".map(Floor::find);
    let tasks: Vec<TaskId> = (0..pickups().len()).map(TaskId).collect();
    let robots: Vec<_> = docks
        .iter()
        .map(|&dock| BestInsertion(Timed(Robot::at(dock))))
        .collect();
    for robot in &robots {
        check_diminishing(robot, &tasks, 3).expect("gains diminish");
    }

    let agents = robots
        .into_iter()
        .enumerate()
        .map(|(id, robot)| Agent::new(AgentId(id), docks.len(), tasks.len(), 3, robot))
        .collect();

    // Wi-Fi in a warehouse: a quarter of all messages get lost, the rest arrive late.
    let network = Network {
        seed: 42,
        max_delay: 3,
        loss_per_mille: 250,
        ..Network::default()
    };
    let mut simulation = Simulation::new(agents, network);
    let ticks = simulation
        .run_until_settled(1_000)
        .expect("the robots agree");

    let assignment = simulation.assignment();
    assert!(assignment.is_conflict_free());
    println!("settled after {ticks} rounds, {:?}", simulation.stats());

    for agent in simulation.agents() {
        let route = &agent.valuation().0;
        let visits = route
            .schedule(agent.bundle().path())
            .expect("bundles are feasible");
        let plan: Vec<String> = visits
            .iter()
            .map(|visit| {
                format!(
                    "pick-up {} done at {}s (+{})",
                    visit.task.index(),
                    visit.finish,
                    visit.reward
                )
            })
            .collect();
        println!("robot {}: {}", agent.id().index(), plan.join(", "));
    }

    println!("total value: {}", assignment.value());
}
