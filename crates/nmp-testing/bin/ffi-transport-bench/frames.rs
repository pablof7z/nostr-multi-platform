// ffi-transport-bench/frames.rs
//
// Deterministic frame-buffer generation for the bench workload.
//
// All frames are pseudo-random bytes generated from a shared PRNG seed so
// both lanes receive byte-identical sequences and the comparison is fair.

use super::rng::Rng;

/// Generate a deterministic frame buffer of `size` bytes from `rng`.
/// The content is synthetic (pseudo-random bytes); transport carries opaque bytes.
pub fn make_frame(rng: &mut Rng, size: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(size);
    for _ in 0..(size / 8) {
        let v = rng.next_u64();
        buf.extend_from_slice(&v.to_le_bytes());
    }
    // Fill any remainder
    let rem = size % 8;
    if rem > 0 {
        let v = rng.next_u64();
        buf.extend_from_slice(&v.to_le_bytes()[..rem]);
    }
    buf
}

/// Generate `count` frame buffers of sizes uniformly distributed in [min, max).
pub fn make_frames(seed: u64, count: usize, min: usize, max: usize) -> Vec<Vec<u8>> {
    let mut rng = Rng::new(seed);
    (0..count)
        .map(|_| {
            let size = rng.next_range(min, max);
            make_frame(&mut rng, size)
        })
        .collect()
}
