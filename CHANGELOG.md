# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `ccbba`: coupled-constraint CBBA, for tasks that depend on or exclude each other, with
  optimistic and pessimistic bidding (`Constraints`, `Optimism`).

## [0.1.0] - 2026-11-22

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
