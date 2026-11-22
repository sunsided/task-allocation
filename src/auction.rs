//! Auction baselines: sequential single-item auctions and combinatorial auctions.
//!
//! Both have an auctioneer: one process that collects every bid and announces the winners.
//! They need fewer rounds of communication than consensus, but a single point that every
//! agent must reach. Use them for comparison, or as a fallback when a central station is
//! available.
//!
//! - [`sequential()`]: tasks go up for auction one at a time, in a fixed order. Every agent
//!   bids its marginal gain on the current task, given what it already won, and the highest
//!   bid wins. Cheap, but the order matters: a task auctioned early can go to an agent that
//!   would have been more useful elsewhere.
//! - [`combinatorial()`]: every agent bids on whole bundles, and the auctioneer picks at most
//!   one bundle per agent, with no task in two of them, maximizing the total
//!   ([`winner_determination`]). Agents can express that tasks are worth more together, at
//!   the price of a search that grows with the number of bids.

mod combinatorial;
mod sequential;

pub use combinatorial::{BundleBid, bundle_bids, combinatorial, winner_determination};
pub use sequential::sequential;
