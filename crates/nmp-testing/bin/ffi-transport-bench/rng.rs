// ffi-transport-bench/rng.rs
//
// Minimal deterministic xoshiro256** PRNG — no dependencies, identical seed
// shared across both lanes so they process byte-identical frame sequences.

pub struct Rng {
    state: [u64; 4],
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // splitmix64 to initialize from a single seed
        let s0 = splitmix64(seed);
        let s1 = splitmix64(s0);
        let s2 = splitmix64(s1);
        let s3 = splitmix64(s2);
        Self {
            state: [s0, s1, s2, s3],
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let result = self.state[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.state[1] << 17;
        self.state[2] ^= self.state[0];
        self.state[3] ^= self.state[1];
        self.state[1] ^= self.state[2];
        self.state[0] ^= self.state[3];
        self.state[2] ^= t;
        self.state[3] = self.state[3].rotate_left(45);
        result
    }

    /// Returns a usize in [lo, hi).
    pub fn next_range(&mut self, lo: usize, hi: usize) -> usize {
        let range = (hi - lo) as u64;
        lo + (self.next_u64() % range) as usize
    }
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
