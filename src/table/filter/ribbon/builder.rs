#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use super::error::{BuildError, ConstructionFailure};
use super::filter::RibbonFilter;
use super::hashing::{SplitMix64, StandardEquation, standard_equation_from_hash};
use super::params::{Mode, Params};

#[derive(Debug, Clone)]
pub struct RibbonBuilder {
    params: Params,
}

impl RibbonBuilder {
    pub fn new(params: Params) -> Result<Self, BuildError> {
        params.validate().map_err(BuildError::InvalidParams)?;
        Ok(Self { params })
    }

    pub fn params(&self) -> Params {
        self.params
    }

    /// Build a Ribbon filter from already-hashed keys (each `u64` is a
    /// stable key hash the caller precomputed). Verbatim seed, no retry.
    ///
    /// Used by BuRR when the LSM has already computed a stable u64
    /// hash for each key (via `crate::hash::hash64` / xxh3); the ribbon
    /// feeds those hashes straight into the banded solver.
    pub(crate) fn build_with_seed_verbatim_from_hashes(
        &self,
        hashes: &[u64],
        seed: u64,
        m: usize,
    ) -> Result<RibbonFilter, BuildError> {
        self.params.validate().map_err(BuildError::InvalidParams)?;
        self.build_once_core(hashes, None, m, seed)
            .map_err(|failure| BuildError::ConstructionFailed {
                final_m: m,
                attempts: 1,
                last_failure: failure,
            })
    }

    /// Build a Ribbon mapping each hashed key to a caller-supplied r-bit
    /// value (a *retrieval* ribbon) instead of a hash-derived membership
    /// fingerprint. Verbatim seed, no retry.
    ///
    /// `values[i]` is the value stored for `hashes[i]`; both slices must be
    /// the same length and each value must already fit in `r` bits. The band
    /// placement (`coeff`, `start`) is still derived from the key hash, so
    /// the solve is identical to the membership path apart from the RHS — a
    /// later dot-product query recovers `values[i]` exactly for a key in the
    /// set (garbage for an absent key, which the caller verifies separately).
    pub(crate) fn build_with_seed_verbatim_from_values(
        &self,
        hashes: &[u64],
        values: &[u64],
        seed: u64,
        m: usize,
    ) -> Result<RibbonFilter, BuildError> {
        self.params.validate().map_err(BuildError::InvalidParams)?;
        self.build_once_core(hashes, Some(values), m, seed)
            .map_err(|failure| BuildError::ConstructionFailed {
                final_m: m,
                attempts: 1,
                last_failure: failure,
            })
    }

    /// Build a single ribbon over pre-computed key hashes.
    ///
    /// Used by BuRR through `build_with_seed_verbatim_from_hashes` (RHS =
    /// hash-derived fingerprint, `values = None`) and
    /// `build_with_seed_verbatim_from_values` (RHS = caller locator,
    /// `values = Some`). The band placement and Gaussian-elimination solve
    /// are shared verbatim between the two paths — only the per-key RHS
    /// differs — so the membership and retrieval ribbons cannot drift.
    fn build_once_core(
        &self,
        hashes: &[u64],
        values: Option<&[u64]>,
        m: usize,
        seed: u64,
    ) -> Result<RibbonFilter, ConstructionFailure> {
        debug_assert!(m >= self.params.w);
        debug_assert!(
            values.is_none_or(|values| values.len() == hashes.len()),
            "retrieval RHS values must be parallel to hashes",
        );

        let fp_mask = self.params.fingerprint_mask();
        let mut occupied = vec![false; m];
        let mut coeff_lo = vec![0u64; m];
        let mut coeff_hi = vec![0u64; m];
        // One row of the solution matrix is one word: `Params::MAX_R` caps the
        // fingerprint at 64 bits, so the RHS and the running row value stay in
        // registers through the whole solve.
        let mut rhs = vec![0u64; m];

        let layer_params = Params { m, ..self.params };

        for (key_index, hash) in hashes.iter().enumerate() {
            let equation: StandardEquation =
                standard_equation_from_hash(*hash, seed, &layer_params);

            let mut i = equation.start;
            let mut c_lo = equation.coeff_lo;
            let mut c_hi = equation.coeff_hi;
            // Retrieval ribbon: the caller's r-bit value replaces the
            // hash-derived fingerprint as the RHS. The band (coeff/start) is
            // still hash-derived, so the solve is unchanged; only what it
            // solves *for* differs.
            let mut b = match values {
                None => equation.fingerprint,
                Some(values) => values[key_index] & fp_mask,
            };

            if i >= m {
                return Err(ConstructionFailure::OutOfBounds {
                    key_index: Some(key_index),
                    row_index: i,
                    m,
                });
            }

            loop {
                if !occupied[i] {
                    occupied[i] = true;
                    coeff_lo[i] = c_lo;
                    coeff_hi[i] = c_hi;
                    rhs[i] = b;
                    break;
                }

                c_lo ^= coeff_lo[i];
                c_hi ^= coeff_hi[i];
                b ^= rhs[i];

                if c_lo == 0 && c_hi == 0 {
                    if b == 0 {
                        break;
                    }
                    return Err(ConstructionFailure::InconsistentEquation {
                        key_index,
                        row_index: i,
                    });
                }

                let shift = if c_lo != 0 {
                    c_lo.trailing_zeros() as usize
                } else {
                    64 + c_hi.trailing_zeros() as usize
                };
                i += shift;
                if i >= m {
                    return Err(ConstructionFailure::OutOfBounds {
                        key_index: Some(key_index),
                        row_index: i,
                        m,
                    });
                }
                if shift >= 64 {
                    c_lo = c_hi >> (shift - 64);
                    c_hi = 0;
                } else if shift > 0 {
                    c_lo = (c_lo >> shift) | (c_hi << (64 - shift));
                    c_hi >>= shift;
                }
            }
        }

        let mut z = vec![0u64; m];
        if matches!(self.params.mode, Mode::Homogeneous) {
            let mut rng = SplitMix64::new(seed ^ 0xD1B5_4A32_D192_ED03);
            for (slot, is_occupied) in z.iter_mut().zip(occupied.iter()) {
                if *is_occupied {
                    continue;
                }
                *slot = rng.next_u64() & fp_mask;
            }
        }

        for i in (0..m).rev() {
            if !occupied[i] {
                continue;
            }
            // The row value is accumulated in a register and stored once. The
            // set bits above the diagonal are walked in place: collecting them
            // into a `Vec` was a heap allocation per row, and XORing straight
            // into `z[i]` made every bit a store the next one had to reload.
            // Each bit names a row strictly below this one (the diagonal bit is
            // masked off), so every source is already final.
            let mut row = rhs[i];
            let mut upper_lo = coeff_lo[i] & !1u64;
            let mut upper_hi = coeff_hi[i];
            while upper_lo != 0 || upper_hi != 0 {
                let offset = if upper_lo != 0 {
                    let bit = upper_lo.trailing_zeros() as usize;
                    upper_lo &= upper_lo - 1;
                    bit
                } else {
                    let bit = upper_hi.trailing_zeros() as usize;
                    upper_hi &= upper_hi - 1;
                    64 + bit
                };
                let row_index = i + offset;
                let Some(other) = z.get(row_index) else {
                    return Err(ConstructionFailure::OutOfBounds {
                        key_index: None,
                        row_index,
                        m,
                    });
                };
                row ^= *other;
            }
            z[i] = row & fp_mask;
        }
        let mut built_params = self.params;
        built_params.m = m;
        built_params.seed = seed;

        Ok(RibbonFilter::new(built_params, z))
    }
}
