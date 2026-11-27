//! Local settle detection against the simulator's global verdict: once a team settles,
//! every agent's detector follows within its span, and on lossy networks the detector can
//! report settled while the agent still holds the wrong bundle.

use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
use task_allocation::sim::{Network, Participant, Simulation};
use task_allocation::{AgentId, Bundle, TaskId, acbba, cbba};

use crate::acbba::lossless;
use crate::cbba::{MAX_TEAM, SETTLE_TICKS, network};
use crate::support::{self, Instance, instance_with};

/// A CBBA team for `instance`.
fn cbba_team(instance: &Instance) -> Vec<cbba::Agent<support::Agent>> {
    let size = instance.agents.len();

    instance
        .agents
        .iter()
        .cloned()
        .enumerate()
        .map(|(id, valuation)| {
            cbba::Agent::new(
                AgentId(id),
                size,
                instance.tasks,
                instance.capacity,
                valuation,
            )
        })
        .collect()
}

/// An ACBBA team for `instance`, following the 2011 table and re-announcing every 10
/// ticks, which repairs what lost messages leave behind.
fn acbba_team(instance: &Instance) -> Vec<acbba::Agent<support::Agent>> {
    instance
        .agents
        .iter()
        .cloned()
        .enumerate()
        .map(|(id, valuation)| {
            acbba::Agent::new(AgentId(id), instance.tasks, instance.capacity, valuation)
                .announce_every(10)
        })
        .collect()
}

/// The last tick at which a partition of `network` is active; zero without partitions.
fn healed(network: &Network) -> u64 {
    network
        .partitions
        .iter()
        .map(|partition| partition.until)
        .max()
        .unwrap_or(0)
}

/// How one run's local verdicts compared with the global one.
#[derive(Clone, Copy, Debug, Default)]
struct Verdicts {
    /// Whether the team settled at all within the tick bound.
    settled: bool,

    /// Agent-ticks at which an agent reported settled while holding a bundle other than
    /// its final one: the mistake that matters, since the agent would act on it.
    wrong_early: u64,

    /// Ticks from the global settle until every agent reported settled.
    late: u64,
}

/// Runs `simulation` until it settles or `bound` ticks pass, then until every agent
/// reports settled under `local` with `span`, comparing each tick's local verdicts with
/// the final bundles.
fn compare<A: Participant>(
    mut simulation: Simulation<A>,
    bound: u64,
    span: u64,
    local: impl Fn(&A, u64) -> bool,
) -> Verdicts {
    let mut history: Vec<Vec<(bool, Bundle<TaskId>)>> = Vec::new();
    let mut ticks = 0;
    while !simulation.is_settled() && ticks < bound {
        simulation.step();
        ticks += 1;
        history.push(
            simulation
                .agents()
                .iter()
                .map(|agent| (local(agent, span), agent.bundle().clone()))
                .collect(),
        );
    }

    if !simulation.is_settled() {
        return Verdicts::default();
    }

    let mut late = 0;
    while !simulation.agents().iter().all(|agent| local(agent, span)) {
        simulation.step();
        late += 1;
    }

    let finals: Vec<&Bundle<TaskId>> = simulation.agents().iter().map(A::bundle).collect();
    let wrong_early = history
        .iter()
        .flat_map(|tick| tick.iter().zip(&finals))
        .filter(|((settled, bundle), last)| *settled && bundle != **last)
        .count();

    Verdicts {
        settled: true,
        wrong_early: u64::try_from(wrong_early).expect("few agent-ticks"),
        late,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Once a CBBA team settles, every agent's detector reports settled within its span,
    /// over lossy, delayed and partitioned networks.
    #[test]
    fn cbba_detectors_follow_a_settled_team(
        (instance, network) in instance_with(MAX_TEAM).prop_flat_map(|instance| {
            let agents = instance.agents.len();
            (Just(instance), network(agents))
        }),
        span in 1_u64..30,
    ) {
        let bound = healed(&network) + SETTLE_TICKS;
        let simulation = Simulation::new(cbba_team(&instance), network);
        let verdicts = compare(simulation, bound, span, cbba::Agent::is_locally_settled);

        prop_assert!(verdicts.settled);
        prop_assert!(verdicts.late <= span, "{:?}", verdicts);
    }

    /// Once an ACBBA team settles without losses, every agent's detector reports settled
    /// within its span, although re-announcements keep moving bid times.
    #[test]
    fn acbba_detectors_follow_a_settled_team(
        (instance, network) in instance_with(MAX_TEAM).prop_flat_map(|instance| {
            let agents = instance.agents.len();
            (Just(instance), lossless(agents))
        }),
        span in 1_u64..30,
    ) {
        let simulation = Simulation::new(acbba_team(&instance), network);
        let verdicts = compare(simulation, SETTLE_TICKS, span, acbba::Agent::is_locally_settled);

        prop_assume!(verdicts.settled, "ACBBA does not always settle");
        prop_assert!(verdicts.late <= span, "{:?}", verdicts);
    }
}

/// Prints how often CBBA and ACBBA detectors with various spans report settled while their
/// agent still holds a wrong bundle, and how long they lag behind the global verdict, on
/// lossless and lossy networks. Run with `LOCAL_SETTLE_CASES=2000`; the default is 200.
#[test]
#[ignore = "an experiment; run on demand"]
fn mistake_rates() {
    let cases: usize = std::env::var("LOCAL_SETTLE_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .unwrap_or(200);

    for (name, lossy) in [("lossless", false), ("lossy", true)] {
        let strategy = instance_with(MAX_TEAM).prop_flat_map(move |instance| {
            let agents = instance.agents.len();
            let network = if lossy {
                network(agents).boxed()
            } else {
                lossless(agents).boxed()
            };
            (Just(instance), network)
        });

        for span in [1, 2, 5, 10, 20, 50] {
            let cbba = tally(&strategy, cases, |instance, network| {
                let bound = healed(&network) + SETTLE_TICKS;
                let simulation = Simulation::new(cbba_team(instance), network);
                compare(simulation, bound, span, cbba::Agent::is_locally_settled)
            });
            let acbba = tally(&strategy, cases, |instance, network| {
                let bound = healed(&network) + SETTLE_TICKS;
                let simulation = Simulation::new(acbba_team(instance), network);
                compare(simulation, bound, span, acbba::Agent::is_locally_settled)
            });

            println!("cbba  {name:>8} span {span:>2}: {cbba}");
            println!("acbba {name:>8} span {span:>2}: {acbba}");
        }
    }
}

/// Runs `judge` on `cases` cases drawn deterministically from `strategy`, and sums up the
/// verdicts of the runs that settled.
fn tally(
    strategy: &impl Strategy<Value = (Instance, Network)>,
    cases: usize,
    judge: impl Fn(&Instance, Network) -> Verdicts,
) -> String {
    let mut runner = TestRunner::deterministic();
    let (mut wrong_runs, mut wrong_ticks, mut late, mut settled) = (0_u32, 0, 0, 0_u32);
    for _ in 0..cases {
        let (instance, network) = strategy.new_tree(&mut runner).expect("generates").current();
        let verdicts = judge(&instance, network);
        if !verdicts.settled {
            continue;
        }

        settled += 1;
        wrong_runs += u32::from(verdicts.wrong_early > 0);
        wrong_ticks += verdicts.wrong_early;
        late += verdicts.late;
    }

    let mean_late = f64::from(u32::try_from(late).expect("small")) / f64::from(settled.max(1));

    format!(
        "wrong early in {wrong_runs}/{settled} runs ({wrong_ticks} agent-ticks), \
         {mean_late:.1} ticks late on average"
    )
}

/// A partition looks just like agreement: two agents cut off from each other both take the
/// only task, and both detectors report settled while the team holds it twice. Once the
/// partition heals, the stronger bid wins and the team settles for real.
#[test]
fn partitions_look_settled() {
    let coverage = |weight| {
        support::Agent::Coverage(support::Coverage {
            covers: vec![1],
            weights: vec![weight],
            allowed: 1,
        })
    };
    let instance = Instance {
        agents: vec![coverage(5), coverage(7)],
        tasks: 1,
        capacity: 1,
    };
    let network = Network {
        partitions: vec![task_allocation::sim::Partition {
            from: 0,
            until: 50,
            groups: vec![0, 1],
        }],
        ..Network::default()
    };
    let mut simulation = Simulation::new(cbba_team(&instance), network);
    for _ in 0..20 {
        simulation.step();
    }

    assert!(
        simulation
            .agents()
            .iter()
            .all(|agent| agent.is_locally_settled(10) && agent.bundle().len() == 1)
    );
    assert!(!simulation.is_settled());

    simulation.run_until_settled(200).expect("settles");
    assert!(simulation.agents()[0].bundle().is_empty());
}

/// An agent with nothing to win counts quiet from its first bundle phase, so a team that is
/// settled before it starts is locally settled after exactly `span` phases.
#[test]
fn idle_agents_count_from_the_start() {
    let idle = support::Agent::Coverage(support::Coverage {
        covers: vec![1],
        weights: vec![5],
        allowed: 0,
    });
    let mut cbba = cbba::Agent::new(AgentId(0), 1, 1, 1, idle.clone());
    let mut acbba = acbba::Agent::new(AgentId(0), 1, 1, idle);
    for now in 0..3 {
        cbba.build_bundle();
        let _ = acbba.build_bundle(now);
    }

    assert_eq!(cbba.quiet_rounds(), 3);
    assert_eq!(acbba.quiet_rounds(), 3);
}
