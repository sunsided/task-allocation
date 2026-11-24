//! A search for the smallest ACBBA team that fails to settle without any message loss:
//! the first reachable state, over growing teams and task counts, from which lossless
//! rounds never bring the team to agreement.
//!
//! Two agents over two and over three tasks, 40 generated instances each and two rounds
//! per agent, never stall under the 2011 table. Three agents are out of reach: every bid
//! carries the time it was made, so states hardly ever repeat, and a single round per agent
//! on a three-agent line did not finish within an hour. The smallest known failure, five
//! agents on a sparse network, comes from the seeded comparison matrix instead (issue #33).

use task_allocation::AgentId;
use task_allocation::acbba::{Agent, Rules};
use task_allocation::sim::Topology;

use super::{Bounds, State, explore};
use crate::explore::cbba::instances;
use crate::support::{self, Instance};

/// Rounds a closing run may take before it counts as never settling.
const CLOSE_LIMIT: u32 = 200;

/// An ACBBA team for `instance` under `rules`, without re-announcing.
fn team(instance: &Instance, rules: Rules) -> Vec<Agent<support::Agent>> {
    instance
        .agents
        .iter()
        .cloned()
        .enumerate()
        .map(|(id, valuation)| {
            Agent::new(AgentId(id), instance.tasks, instance.capacity, valuation).with_rules(rules)
        })
        .collect()
}

/// Whether `state`'s closing run fails to settle; describes the failure if so, saying
/// whether the network fell quiet without agreement or kept sending.
fn stall(state: &State<Agent<support::Agent>>, topology: &Topology) -> Option<String> {
    let mut closing = state.clone();
    if closing.close(topology, CLOSE_LIMIT).is_some() {
        return None;
    }

    for agent in 0..closing.agents.len() {
        closing.act(agent, topology);
    }
    let verdict = if closing.in_flight.is_empty() {
        "falls quiet without agreeing"
    } else {
        "never falls quiet"
    };

    Some(format!(
        "{verdict}\nschedule:\n  {}\nagents after {CLOSE_LIMIT} closing rounds: {:#?}",
        state.trace.join("\n  "),
        closing.agents
    ))
}

/// One team shape to search: agents, tasks, topology, rounds per agent, and how many
/// generated instances.
type Shape = (usize, usize, Topology, u32, usize);

/// The teams to search, smallest first: two agents over two and over three tasks, two
/// rounds each.
fn shapes() -> Vec<Shape> {
    vec![
        (2, 2, Topology::Complete, 2, 40),
        (2, 3, Topology::Complete, 2, 40),
    ]
}

/// Searches the shapes in order, `instances` generated instances each, for a state from
/// which the 2011 table never settles, and prints the first found.
#[test]
#[ignore = "a search; run with --ignored"]
fn smallest_stall() {
    for (agents, tasks, topology, acts, count) in shapes() {
        let bounds = Bounds { acts, losses: 0 };
        for (index, instance) in instances(agents, tasks, count).iter().enumerate() {
            let start = State::new(team(instance, Rules::Infotech2011));
            let found = explore(start, &topology, bounds, |state| {
                stall(state, &topology).map_or(Ok(()), Err)
            });

            if let Err(stall) = found {
                println!(
                    "{agents} agents, {tasks} tasks, {topology:?}, instance {index}: {instance:?}\n{stall}"
                );
                return;
            }
        }

        println!("{agents} agents, {tasks} tasks, {topology:?}, {acts} rounds each: no stall");
    }
}
