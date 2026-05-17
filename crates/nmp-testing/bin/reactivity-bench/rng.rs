pub(crate) fn seed_for(name: &str) -> u64 {
    let mut seed = 0xcbf2_9ce4_8422_2325_u64;
    for byte in name.as_bytes() {
        seed ^= *byte as u64;
        seed = seed.wrapping_mul(0x0000_0100_0000_01b3);
    }
    seed
}

#[derive(Clone, Copy)]
pub(crate) struct Lcg {
    pub(crate) state: u64,
}

impl Lcg {
    pub(crate) fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    pub(crate) fn next_mod(&mut self, modulus: u64) -> u64 {
        if modulus == 0 {
            0
        } else {
            self.next() % modulus
        }
    }
}
