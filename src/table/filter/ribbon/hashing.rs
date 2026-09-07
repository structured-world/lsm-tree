use super::params::{Mode, Params};

const MIX_CONST: u64 = 0x9E37_79B9_7F4A_7C15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StandardEquation {
    pub(crate) start: usize,
    pub(crate) coeff_lo: u64,
    pub(crate) coeff_hi: u64,
    /// Hash-derived right-hand side, masked to the low `r` bits. Zero in
    /// homogeneous mode, which solves against an all-zero RHS.
    pub(crate) fingerprint: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub(crate) fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(MIX_CONST);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

fn fastrange_u64(x: u64, range: usize) -> usize {
    ((x as u128 * range as u128) >> 64) as usize
}

#[inline]
pub(crate) fn start_position_from_stream(next_word: u64, m: usize, w: usize) -> usize {
    let start_range = m - w + 1;
    // TODO: add optional boundary smash strategy here.
    // TODO: add fractional-r/ICML tuned layout hooks once layout work starts.
    fastrange_u64(next_word, start_range)
}

/// Compute the equation directly from a pre-computed key hash.
///
/// The LSM filter framework hands in keys already hashed to a stable
/// `u64` (xxh3 / `crate::hash::hash64`), so the ribbon never hashes keys
/// itself — this is the sole equation entry point.
///
/// The fingerprint travels in the returned struct rather than through an
/// out-parameter: `r` is capped at 64 by [`Params::validate`], so it is one
/// word, and both the build solve and the probe want it in a register.
#[expect(
    clippy::inline_always,
    reason = "called per layer on the BuRR filter probe hot path; inlining lets LLVM fold the \
              SplitMix stream into the caller"
)]
#[inline(always)]
pub(crate) fn standard_equation_from_hash(
    base_hash: u64,
    seed: u64,
    params: &Params,
) -> StandardEquation {
    let stream_seed = (base_hash ^ seed).wrapping_mul(MIX_CONST);
    let mut stream = SplitMix64::new(stream_seed);

    let start = start_position_from_stream(stream.next_u64(), params.m, params.w);

    let (coeff_lo, coeff_hi) = if params.w <= 64 {
        let width_mask = if params.w == 64 {
            u64::MAX
        } else {
            (1u64 << params.w) - 1
        };
        ((stream.next_u64() & width_mask) | 1, 0)
    } else {
        let lo = stream.next_u64();
        let hi_bits = params.w - 64;
        let hi_mask = if hi_bits == 64 {
            u64::MAX
        } else {
            (1u64 << hi_bits) - 1
        };
        (lo | 1, stream.next_u64() & hi_mask)
    };

    let fingerprint = if matches!(params.mode, Mode::Homogeneous) {
        0
    } else {
        stream.next_u64() & params.fingerprint_mask()
    };

    StandardEquation {
        start,
        coeff_lo,
        coeff_hi,
        fingerprint,
    }
}
