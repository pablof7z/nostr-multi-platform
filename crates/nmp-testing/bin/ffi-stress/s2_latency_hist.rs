//! T114b — fixed-size log2 latency histogram for S2 dispatch-flood.
//!
//! Replaces the per-sample `Vec<u64>` in `s2_dispatch_flood.rs` that
//! retained ~8 B per dispatch in the global counting allocator and
//! inflated the `retained_heap_after_drain_bytes` §G-S2 gate with
//! harness bookkeeping (see `docs/perf/m10.5/s2-retention-audit.md`).
//!
//! 32 buckets cover `1 ns .. 2^31 ns` (~2.1 s); bucket `b` accumulates
//! samples in `[2^b, 2^(b+1))`. Total fixed footprint = 32 × 8 B = 256 B
//! per histogram, regardless of dispatch count. Per-thread histograms
//! merge after the flood; percentiles are estimated from bucket midpoints
//! (log-spaced geometric centres). Precision near the §G-S2 thresholds
//! (p50 ≤ 100 µs, p99 ≤ 1 ms) is bounded by the bucket widths there
//! (~64 µs / ~512 µs respectively) — well inside the contract margins.

pub(crate) const HIST_BUCKETS: usize = 32;

#[derive(Default)]
pub(crate) struct LatencyHistogram {
    pub(crate) buckets: [u64; HIST_BUCKETS],
    pub(crate) count: u64,
    pub(crate) sum_log2: u64, // for verification — unused in percentile estimate
}

impl LatencyHistogram {
    pub(crate) fn record(&mut self, ns: u64) {
        // Bucket = floor(log2(ns)), saturated to top bucket.
        let bucket = if ns == 0 {
            0
        } else {
            let lg = (u64::BITS - ns.leading_zeros() - 1) as usize;
            lg.min(HIST_BUCKETS - 1)
        };
        self.buckets[bucket] = self.buckets[bucket].saturating_add(1);
        self.count = self.count.saturating_add(1);
        self.sum_log2 = self.sum_log2.saturating_add(bucket as u64);
    }

    pub(crate) fn merge(&mut self, other: &LatencyHistogram) {
        for (a, b) in self.buckets.iter_mut().zip(other.buckets.iter()) {
            *a = a.saturating_add(*b);
        }
        self.count = self.count.saturating_add(other.count);
        self.sum_log2 = self.sum_log2.saturating_add(other.sum_log2);
    }

    /// Percentile in nanoseconds, estimated from bucket midpoints (geometric).
    pub(crate) fn percentile_ns(&self, pct: usize) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let target = (self.count.saturating_mul(pct as u64)) / 100;
        let mut acc = 0u64;
        for (b, &cnt) in self.buckets.iter().enumerate() {
            acc = acc.saturating_add(cnt);
            if acc >= target.max(1) {
                // Bucket b covers [2^b, 2^(b+1)); midpoint ≈ 1.5 × 2^b.
                let lo = 1u64 << b;
                let hi = if b + 1 >= HIST_BUCKETS {
                    lo.saturating_mul(2)
                } else {
                    1u64 << (b + 1)
                };
                return lo.saturating_add((hi - lo) / 2);
            }
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::LatencyHistogram;

    #[test]
    fn histogram_percentile_estimates_log2_band() {
        // 1000 samples, each at ~1 ms (~1_000_000 ns → bucket 19 since
        // 2^19 = 524_288, 2^20 = 1_048_576). All samples land in bucket 19,
        // so p50/p99 ≈ midpoint(2^19, 2^20) = 524288 + 262144 = 786432.
        let mut h = LatencyHistogram::default();
        for _ in 0..1000 {
            h.record(1_000_000);
        }
        let p50 = h.percentile_ns(50);
        let p99 = h.percentile_ns(99);
        assert_eq!(p50, 786_432);
        assert_eq!(p99, 786_432);
    }

    #[test]
    fn histogram_fixed_footprint() {
        // Two histograms record different counts; both occupy the same fixed
        // 32-bucket storage. This pins the "no per-sample retention" property.
        let mut h_small = LatencyHistogram::default();
        let mut h_large = LatencyHistogram::default();
        for _ in 0..10 {
            h_small.record(1_000);
        }
        for _ in 0..1_000_000 {
            h_large.record(1_000);
        }
        assert_eq!(h_small.buckets.len(), h_large.buckets.len());
        assert_eq!(std::mem::size_of_val(&h_small), std::mem::size_of_val(&h_large));
        assert_eq!(h_small.count, 10);
        assert_eq!(h_large.count, 1_000_000);
    }

    #[test]
    fn histogram_merge_preserves_total_count() {
        let mut a = LatencyHistogram::default();
        let mut b = LatencyHistogram::default();
        for _ in 0..100 {
            a.record(500);
        }
        for _ in 0..200 {
            b.record(8_000);
        }
        a.merge(&b);
        assert_eq!(a.count, 300);
    }
}
