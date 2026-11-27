//! Benchmark scenarios from the literature, each with a fast property and an ignored
//! report that prints how the protocols compare. Run a report with
//! `cargo test --release --test it <name> -- --ignored --nocapture`.

mod margin;

use task_allocation::TaskId;
use task_allocation::sim::{Participant, Simulation};

/// How one run ended, in the units the reports print.
#[derive(Clone, Copy, Debug, Default)]
pub struct Outcome {
    /// Whether the team settled within the tick limit.
    pub settled: bool,

    /// Ticks until the team settled, or the limit.
    pub ticks: u64,

    /// Bytes broadcast over the run.
    pub bytes: u64,

    /// Copies delivered over the run.
    pub delivered: u64,

    /// Delivered copies that changed their receiver.
    pub useful: u64,

    /// Tasks the final assignment holds.
    pub assigned: u64,
}

/// Runs `simulation` until it settles or `limit` ticks pass, and sums up the run over
/// `tasks` tasks.
pub fn measure<A: Participant>(mut simulation: Simulation<A>, tasks: usize, limit: u64) -> Outcome {
    let settled = simulation.run_until_settled(limit);
    let stats = simulation.stats();
    let assignment = simulation.assignment();
    let assigned = (0..tasks)
        .filter(|&task| assignment.holder(TaskId(task)).is_some())
        .count();

    Outcome {
        settled: settled.is_some(),
        ticks: settled.unwrap_or(limit),
        bytes: stats.bytes,
        delivered: stats.delivered,
        useful: stats.useful,
        assigned: u64::try_from(assigned).expect("few tasks"),
    }
}

/// Totals over many runs of one protocol, for one row of a report.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tally {
    /// Runs measured.
    runs: u64,

    /// The sums over the runs that settled.
    settled: Outcome,

    /// Runs that settled.
    settled_runs: u64,
}

impl Tally {
    /// Adds one run.
    pub fn add(&mut self, outcome: Outcome) {
        self.runs += 1;
        if !outcome.settled {
            return;
        }

        self.settled_runs += 1;
        self.settled.ticks += outcome.ticks;
        self.settled.bytes += outcome.bytes;
        self.settled.delivered += outcome.delivered;
        self.settled.useful += outcome.useful;
        self.settled.assigned += outcome.assigned;
    }
}

impl core::fmt::Display for Tally {
    /// Settled share, mean ticks, bytes per assigned task and the share of useful copies,
    /// the last three over the runs that settled; "not run" without runs.
    #[expect(
        clippy::cast_precision_loss,
        reason = "printed ratios need no more than f64's 53 bits"
    )]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let ratio = |part: u64, whole: u64| {
            if whole == 0 {
                0.0
            } else {
                part as f64 / whole as f64
            }
        };
        if self.runs == 0 {
            return write!(f, "not run");
        }

        let total = &self.settled;

        write!(
            f,
            "settled {:>5.1}%  ticks {:>7.1}  bytes/task {:>8.1}  useful {:>5.1}%",
            100.0 * ratio(self.settled_runs, self.runs),
            ratio(total.ticks, self.settled_runs),
            ratio(total.bytes, total.assigned),
            100.0 * ratio(total.useful, total.delivered),
        )
    }
}
