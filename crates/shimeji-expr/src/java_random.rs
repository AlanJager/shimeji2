//! Bit-exact [`java.util.Random`](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/util/Random.html).
//!
//! 48-bit LCG: `seed = (seed * 0x5DEECE66D + 0xB) & ((1 << 48) - 1)`.
//! Vectors in `tests/vectors/java_random.json` were captured from JDK 21.

const MULTIPLIER: u64 = 0x0005_DEEC_E66D;
const ADDEND: u64 = 0xB;
const MASK: u64 = (1 << 48) - 1;
const DOUBLE_UNIT: f64 = 1.0 / ((1u64 << 53) as f64);

/// One `java.util.Random` stream. Not thread-safe; the engine holds one per instance.
#[derive(Debug, Clone)]
pub struct JavaRandom {
    seed: u64,
}

impl JavaRandom {
    /// `new Random(seed)`: scramble with the multiplier and keep 48 bits.
    pub fn new(seed: i64) -> Self {
        Self {
            seed: initial_scramble(seed),
        }
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.seed = initial_scramble(seed);
    }

    /// `protected int next(int bits)`.
    pub fn next_bits(&mut self, bits: u32) -> i32 {
        debug_assert!((1..=32).contains(&bits));
        self.seed = self.seed.wrapping_mul(MULTIPLIER).wrapping_add(ADDEND) & MASK;
        (self.seed >> (48 - bits)) as i32
    }

    pub fn next_int(&mut self) -> i32 {
        self.next_bits(32)
    }

    /// `nextInt(bound)`. Panics if `bound` is not positive, matching Java's
    /// `IllegalArgumentException`.
    pub fn next_int_bound(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive");
        if (bound & bound.wrapping_neg()) == bound {
            let bits = self.next_bits(31);
            return ((i64::from(bound) * i64::from(bits)) >> 31) as i32;
        }
        loop {
            let bits = self.next_bits(31);
            let val = bits % bound;
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }

    pub fn next_double(&mut self) -> f64 {
        let high = i64::from(self.next_bits(26)) << 27;
        let low = i64::from(self.next_bits(27));
        (high + low) as f64 * DOUBLE_UNIT
    }

    pub fn next_boolean(&mut self) -> bool {
        self.next_bits(1) != 0
    }
}

fn initial_scramble(seed: i64) -> u64 {
    (seed as u64 ^ MULTIPLIER) & MASK
}
