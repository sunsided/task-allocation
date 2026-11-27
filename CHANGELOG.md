# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/sunsided/task-allocation/compare/v0.1.2...v0.2.0) - 2026-11-27

### Breaking changes

- `sim::Participant::receive` now returns whether the receiver's winners, bids or bundle changed.
- `sim::Participant::message_bytes` is a new required method: a message's nominal size on the wire.
- `sim::Stats` gains `useful`, `bytes` and `collided`, and `sim::Network` gains `collisions` and `clusters`. Struct literals need the new fields, or `..Default::default()`.

### Added

- Add bid warping to CBBA (`cbba::Agent::with_bid_warping`) for gains that do not diminish ([#45](https://github.com/sunsided/task-allocation/pull/45))
- Add BW-TC (`bwtc`), the task consensus of bid-warped CBBA, experimental ([#44](https://github.com/sunsided/task-allocation/pull/44))
- Add an experimental ACBBA (`acbba`) with the 2010, 2011 and 2012 decision tables; the 2011 table is the default
- Add event-driven broadcasting to CBBA (`cbba::Cadence::OnChange`)
- Add local settle detection to CBBA and ACBBA agents (`quiet_rounds`, `is_locally_settled`) ([#46](https://github.com/sunsided/task-allocation/pull/46))
- Add useful-copy and byte counts to the simulator's `Stats`, and `bwtc::Entry::task` ([#47](https://github.com/sunsided/task-allocation/pull/47))
- Add hidden-node collisions and slow links between clusters to the simulated network ([#48](https://github.com/sunsided/task-allocation/pull/48))
- Add benchmark scenarios after AC-DMTA, GACA, CTAS-AMP, JKROS and ISARC 2026 to the test suite ([#49](https://github.com/sunsided/task-allocation/pull/49) to [#53](https://github.com/sunsided/task-allocation/pull/53))

### Known issues

- Timed CCBBA does not check the timing limits between tasks of one agent's own bundle while bidding, so tasks that must start together, such as a two-carrier pickup, can keep a team from settling. The ignored test `scenarios::transport::two_carriers_lift_together` reproduces it.

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
