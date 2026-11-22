//! A small, seedable pseudo-random number generator, so simulations are reproducible
//! without a dependency.

/// The `SplitMix64` generator (Steele, Lea and Flood, 2014): fast, with a 64-bit state, and
/// good enough for simulated delays and losses. Not for cryptography.
#[derive(Clone, Debug)]
pub struct SplitMix64 {
    /// The generator state; advances by a fixed odd constant on every draw.
    state: u64,
}

impl SplitMix64 {
    /// A generator seeded with `seed`.
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next 64 random bits.
    pub const fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);

        z ^ (z >> 31)
    }

    /// A number in `0..bound`; zero when `bound` is zero. The modulo bias is below
    /// `bound / 2^64`, negligible for the small bounds used here.
    pub const fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }

        self.next_u64() % bound
    }
}

/// Unit tests for [`SplitMix64`].
#[cfg(test)]
mod tests {
    use super::SplitMix64;

    /// The first outputs for seed 0 match the reference implementation.
    #[test]
    fn matches_the_reference_sequence() {
        let mut rng = SplitMix64::new(0);

        assert_eq!(rng.next_u64(), 0xe220_a839_7b1d_cdaf);
        assert_eq!(rng.next_u64(), 0x6e78_9e6a_a1b9_65f4);
    }

    /// Bounded draws stay below the bound.
    #[test]
    fn below_stays_in_range() {
        let mut rng = SplitMix64::new(42);

        assert!((0..1000).all(|_| rng.below(7) < 7));
        assert_eq!(rng.below(0), 0);
    }
}
