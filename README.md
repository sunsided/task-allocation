# task-allocation

Decentralized multi-agent task allocation in Rust: agents decide among themselves who does which task, exchanging bids whenever they can communicate, without a central authority.

## Scope

- Consensus-based bundle algorithm (CBBA) and its variants, converging to conflict-free assignments under delayed and intermittent communication
- Sequential and combinatorial auctions as baselines
- Task scores from user-supplied costs, such as travel time, deadlines or resource use

Typical users are robot and rover teams, drone swarms and multi-agent simulations. Timing constraints between agents fit [temporal-control](https://github.com/sunsided/temporal-control), shared resource budgets [resource-timeline](https://github.com/sunsided/resource-timeline), and travel costs can come from [heuristic-search](https://github.com/sunsided/heuristic-search) or [sampling-planning](https://github.com/sunsided/sampling-planning).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/sunsided/task-allocation/blob/main/LICENSE-APACHE))
- MIT license ([LICENSE-MIT](https://github.com/sunsided/task-allocation/blob/main/LICENSE-MIT))

at your option.
