# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/sunsided/task-allocation/compare/v0.1.2...v0.2.0) - 2026-11-27

### Added

- Add hidden-node collisions and cluster rates to the network
- Add local settle detection to CBBA and ACBBA agents ([#46](https://github.com/sunsided/task-allocation/pull/46))
- Add local settle detection to CBBA and ACBBA agents
- Add bid warping to CBBA for gains that do not diminish
- Add BW-TC, the task consensus of bid-warped CBBA
- Add the 2010 and 2012 ACBBA tables and compare them with 2011
- Add an ACBBA implementation that does not yet settle reliably

### Changed

- Rewrap the timed CCBBA module docs and size the scratch path once
- Keep the timing limits among an agent's own tasks in timed CCBBA
- Explain why tours may nail fewer joints than the optimum
- Route nailing tours through a timber frame
- Pin the cooperative transport scenario
- Measure timed CCBBA on precedence chains
- Score open multi-agent routes as a percentage of the optimum
- Give every frame its own backoff slot
- Say that bytes count airtime, lost copies included
- Compare protocols on near-boundary and large-margin teams
- Count useful copies and bytes in the simulator
- Say where bid warping changes diminishing bids
- Describe what the warping tests show, not why Example 2 settles
- Count quiet rounds from an agent's initial view
- Pin BW-TC re-announcements and document helper panics
- Keep the deeper BW-TC exploration within the hour
- Search exhaustively for the smallest ACBBA team that stalls
- Keep the deeper CCBBA exploration within minutes
- Explore every message schedule of tiny CCBBA teams
- Key explored states by 128 bits, and check stability on the explored topology
- Explore every message schedule of tiny CBBA teams
- Stop claiming every re-announcing ACBBA run settles
- Let CBBA agents broadcast only after their tables change
- Mark ACBBA experimental and default to the 2011 table
- Document ACBBA reset and push choices, and classify settled runs
- Test timed CCBBA against a brute-force optimum

### Dependencies

- Bump the version to 0.2.0 for the Participant change

### Removed

- Drop the proptest seeds left by a mutation check

## [0.1.2](https://github.com/sunsided/task-allocation/compare/v0.1.1...v0.1.2) - 2026-11-23

### Added

- Add tasks to, retire tasks from, and remove agents from a running team
- Add a per-phase bundle budget, and reuse the simulator's buffers

### Changed

- Check bids and capacity after team changes, and count plans safely
- Reject budgets that a replan outruns, and release without allocating

## [0.1.1](https://github.com/sunsided/task-allocation/compare/v0.1.0...v0.1.1) - 2026-11-22

### Added

- Add a colony benchmark with allocation counts per round
- Add a colony-sim example, and keep the audience note short

### Changed

- Grow bundles in place without replanning, and test the phases directly
- Score insertions and merge messages without allocating
- Count round allocations in their own test, off the benchmark's clock

## [0.1.0] - 2026-11-23

### Added

- `Valuation` with `best_insertion`, `PathScore` and the `BestInsertion` adapter for
  path-dependent scores, and `check_diminishing` to test that marginal gains diminish.
- `Bundle`: greedy bundle construction from marginal gains, with release of a lost task and
  everything added after it.
- `Assignment`, and centralized references in `centralized`: the sequential greedy assignment
  and the exhaustive optimum for small instances.
- `cbba`: the consensus-based bundle algorithm, with logical clocks instead of synchronized
  timestamps, robust to delayed, lost, duplicated and reordered messages, and opt-in bundle
  replanning (`Replan`).
- `ccbba`: coupled-constraint CBBA for tasks that depend on or exclude each other, with
  optimistic and pessimistic bidding, and timing limits between their starts for agents on
  timed routes (`TimedAgent`).
- `sim`: a deterministic simulated network with seeded delays, losses, topologies and
  partitions, for any protocol implementing `Participant`.
- `auction`: sequential single-item auctions, and combinatorial auctions with exact winner
  determination.
- `route`: timed routes, a ready-made path score from travel times, service windows and
  finish-time rewards (`deadline`, `linear_decay`, `halving`).
- Examples pairing the allocation with temporal-control, resource-timeline, heuristic-search
  and sampling-planning.

[Unreleased]: https://github.com/sunsided/task-allocation/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/sunsided/task-allocation/releases/tag/v0.1.0
