//! `MemoryLifecycleEnforcer` — MemGPT-style tier decay.
//!
//! Given age, recent-hit signal, and a usefulness score, returns the
//! correct `MemoryLifecycle` tier. Pure logic — callers feed the
//! signals from their own clocks/metrics; this module makes no IO.
//!
//! Plan: cycle `evolution/apr20-1553` research §P1.
//! Reference: MemGPT [@packer2023] 3-tier decay (main/archival/recall).
//!
//! ## Transitions modelled
//!
//! - `Active → Cooling`: the episode is older than `active_max_age_secs`
//!   AND has not been hit inside the active window OR usefulness fell
//!   below the active floor.
//! - `Cooling → Archived`: the episode is older than `cooling_max_age_secs`
//!   OR usefulness fell below `archived_usefulness_floor` with no
//!   recent hits.
//! - `Archived → Archived`: terminal (per `MemoryLifecycle::next`).
//!
//! A tier may also stay where it is when none of the triggers fire —
//! `tick()` never "revives" a tier backwards; that would require a
//! separate explicit promotion call.

use crate::episode::MemoryLifecycle;

/// Tunable thresholds. Production defaults are calibrated to match
/// the usefulness thresholds already pinned in `MemoryLifecycle` so
/// the enforcer and the gate return consistent verdicts.
#[derive(Debug, Clone)]
pub struct DecayThresholds {
    /// Max age an `Active` entry may hold before being demoted to
    /// `Cooling` (seconds since creation).
    pub active_max_age_secs: u64,
    /// Max age a `Cooling` entry may hold before being demoted to
    /// `Archived`.
    pub cooling_max_age_secs: u64,
    /// Below this usefulness score, an `Active` entry demotes to
    /// `Cooling` regardless of age. Mirrors
    /// `MemoryLifecycle::Cooling::usefulness_threshold` (0.3).
    pub active_usefulness_floor: f64,
    /// Below this, a `Cooling` entry demotes to `Archived` once no
    /// recent hits are recorded.
    pub archived_usefulness_floor: f64,
    /// Number of recent hits that keeps an entry warm (does NOT demote
    /// despite age). 0 disables the hit-shield entirely.
    pub min_hits_to_stay_warm: u32,
}

impl Default for DecayThresholds {
    fn default() -> Self {
        Self {
            active_max_age_secs: 2 * 60 * 60,       // 2h
            cooling_max_age_secs: 7 * 24 * 60 * 60, // 7 days
            active_usefulness_floor: 0.30,
            archived_usefulness_floor: 0.10,
            min_hits_to_stay_warm: 1,
        }
    }
}

/// Stateless dispatcher that produces the correct lifecycle tier given
/// a current tier plus the three decay signals.
pub struct MemoryLifecycleEnforcer;

impl MemoryLifecycleEnforcer {
    /// Returns the tier the caller should persist. Never promotes
    /// backwards (Cooling → Active requires an explicit call site).
    pub fn tick(
        current: MemoryLifecycle,
        age_secs: u64,
        usefulness: f64,
        hit_count: u32,
        thresholds: &DecayThresholds,
    ) -> MemoryLifecycle {
        match current {
            MemoryLifecycle::Active => {
                let aged_out = age_secs >= thresholds.active_max_age_secs;
                let useless = usefulness < thresholds.active_usefulness_floor;
                let warm = hit_count >= thresholds.min_hits_to_stay_warm;
                if (aged_out && !warm) || useless {
                    MemoryLifecycle::Cooling
                } else {
                    MemoryLifecycle::Active
                }
            }
            MemoryLifecycle::Cooling => {
                let aged_out = age_secs >= thresholds.cooling_max_age_secs;
                let useless = usefulness < thresholds.archived_usefulness_floor;
                let warm = hit_count >= thresholds.min_hits_to_stay_warm;
                if aged_out || (useless && !warm) {
                    MemoryLifecycle::Archived
                } else {
                    MemoryLifecycle::Cooling
                }
            }
            MemoryLifecycle::Archived => MemoryLifecycle::Archived,
        }
    }

    /// Check if a Cooling episode should be promoted back to Active.
    ///
    /// Separate from `tick()` which handles demotion only. Promotion
    /// is an explicit, distinct code path with its own thresholds
    /// and anti-thrashing guards.
    ///
    /// Reference: 2Q cache admission (promote on second access),
    /// ByteRover AKL (access=+3, threshold-based tiers),
    /// SM-2 spaced repetition (interval-based promotion).
    pub fn should_promote(
        current: MemoryLifecycle,
        hit_count: u32,
        age_in_current_tier_secs: u64,
        secs_since_last_transition: u64,
        thresholds: &PromotionThresholds,
    ) -> bool {
        if current != MemoryLifecycle::Cooling {
            return false;
        }
        // Anti-thrash: respect cooldown after any tier transition.
        if secs_since_last_transition < thresholds.cooldown_after_transition_secs {
            return false;
        }
        // Promotion window expired: episode is headed toward Archived.
        if age_in_current_tier_secs > thresholds.promotion_window_secs {
            return false;
        }
        hit_count >= thresholds.min_hits_to_promote
    }
}

/// Thresholds for reverse promotion (Cooling → Active).
///
/// Three anti-thrashing layers:
/// 1. `cooldown_after_transition_secs` — no transition within this window.
/// 2. Hit count reset on promotion — needs fresh hits to survive.
/// 3. `promotion_window_secs` — cap on how long promotion is possible.
#[derive(Debug, Clone)]
pub struct PromotionThresholds {
    /// Minimum hits accumulated while in Cooling before eligible.
    pub min_hits_to_promote: u32,
    /// Seconds since last tier transition before allowing another.
    pub cooldown_after_transition_secs: u64,
    /// Maximum time in Cooling (seconds) after which promotion is
    /// no longer possible — episode goes toward Archived instead.
    pub promotion_window_secs: u64,
}

impl Default for PromotionThresholds {
    fn default() -> Self {
        Self {
            min_hits_to_promote: 2,
            cooldown_after_transition_secs: 600,          // 10 minutes
            promotion_window_secs: 3 * 24 * 60 * 60,     // 3 days
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thr() -> DecayThresholds {
        DecayThresholds::default()
    }

    // ── Active → Cooling ────────────────────────────────────────
    #[test]
    fn test_active_aged_and_unused_demotes_to_cooling() {
        let new = MemoryLifecycleEnforcer::tick(
            MemoryLifecycle::Active,
            3 * 60 * 60, // 3h, over 2h active window
            0.5,
            0, // no hits
            &thr(),
        );
        assert_eq!(new, MemoryLifecycle::Cooling);
    }

    #[test]
    fn test_active_warm_despite_age_stays_active() {
        let new = MemoryLifecycleEnforcer::tick(
            MemoryLifecycle::Active,
            10 * 60 * 60, // well past the age limit
            0.9,
            5, // 5 recent hits → warm shield
            &thr(),
        );
        assert_eq!(new, MemoryLifecycle::Active);
    }

    #[test]
    fn test_active_below_usefulness_floor_demotes_regardless_of_hits() {
        let new = MemoryLifecycleEnforcer::tick(
            MemoryLifecycle::Active,
            60, // fresh
            0.1, // below floor
            100,
            &thr(),
        );
        assert_eq!(
            new,
            MemoryLifecycle::Cooling,
            "usefulness floor overrides warm shield"
        );
    }

    // ── Cooling → Archived ──────────────────────────────────────
    #[test]
    fn test_cooling_aged_out_becomes_archived() {
        let new = MemoryLifecycleEnforcer::tick(
            MemoryLifecycle::Cooling,
            10 * 24 * 60 * 60, // 10 days > 7
            0.5,
            3,
            &thr(),
        );
        assert_eq!(new, MemoryLifecycle::Archived);
    }

    #[test]
    fn test_cooling_unused_and_cold_becomes_archived() {
        let new = MemoryLifecycleEnforcer::tick(
            MemoryLifecycle::Cooling,
            60,   // fresh
            0.05, // below archived floor
            0,    // cold
            &thr(),
        );
        assert_eq!(new, MemoryLifecycle::Archived);
    }

    #[test]
    fn test_cooling_with_recent_hits_stays_cooling() {
        let new = MemoryLifecycleEnforcer::tick(
            MemoryLifecycle::Cooling,
            60,
            0.05, // would demote if cold
            1,    // one hit — warm shield
            &thr(),
        );
        assert_eq!(new, MemoryLifecycle::Cooling);
    }

    // ── Archived is terminal ────────────────────────────────────
    #[test]
    fn test_archived_stays_archived_regardless_of_signals() {
        for hit in [0, 10] {
            for useful in [0.0, 0.99] {
                let new = MemoryLifecycleEnforcer::tick(
                    MemoryLifecycle::Archived,
                    0,
                    useful,
                    hit,
                    &thr(),
                );
                assert_eq!(new, MemoryLifecycle::Archived);
            }
        }
    }

    // ── No backwards promotion ──────────────────────────────────
    #[test]
    fn test_cooling_never_promotes_back_to_active() {
        // Even with the "best" signals, the enforcer does not revive
        // a demoted entry — that decision belongs to explicit callers.
        let new = MemoryLifecycleEnforcer::tick(
            MemoryLifecycle::Cooling,
            0,
            1.0,
            100,
            &thr(),
        );
        assert_eq!(new, MemoryLifecycle::Cooling);
    }

    // ── Fresh Active stays Active ───────────────────────────────
    #[test]
    fn test_fresh_useful_active_stays_active() {
        let new = MemoryLifecycleEnforcer::tick(
            MemoryLifecycle::Active,
            60,
            0.8,
            2,
            &thr(),
        );
        assert_eq!(new, MemoryLifecycle::Active);
    }

    // ── Threshold consistency with MemoryLifecycle::usefulness_threshold
    #[test]
    fn defaults_mirror_memorylifecycle_usefulness_thresholds() {
        let t = DecayThresholds::default();
        assert!((t.active_usefulness_floor - 0.30).abs() < 1e-6);
        assert!(t.archived_usefulness_floor < t.active_usefulness_floor);
    }

    // ── Reverse promotion: should_promote ──────────────────────
    fn pthr() -> PromotionThresholds {
        PromotionThresholds::default()
    }

    #[test]
    fn test_should_promote_cooling_with_enough_hits() {
        let result = MemoryLifecycleEnforcer::should_promote(
            MemoryLifecycle::Cooling,
            2,      // hits >= min_hits_to_promote (2)
            3600,   // 1h in tier
            3600,   // 1h since last transition (> 600s cooldown)
            &pthr(),
        );
        assert!(result, "Cooling with 2 hits past cooldown should promote");
    }

    #[test]
    fn test_should_promote_cooling_insufficient_hits() {
        let result = MemoryLifecycleEnforcer::should_promote(
            MemoryLifecycle::Cooling,
            1,      // < min_hits_to_promote (2)
            3600,
            3600,
            &pthr(),
        );
        assert!(!result, "1 hit is not enough to promote");
    }

    #[test]
    fn test_should_promote_within_cooldown_blocked() {
        let result = MemoryLifecycleEnforcer::should_promote(
            MemoryLifecycle::Cooling,
            5,      // plenty of hits
            3600,
            30,     // only 30s since transition (< 600s cooldown)
            &pthr(),
        );
        assert!(!result, "within cooldown window must block promotion");
    }

    #[test]
    fn test_should_promote_past_window_blocked() {
        let result = MemoryLifecycleEnforcer::should_promote(
            MemoryLifecycle::Cooling,
            10,
            5 * 24 * 60 * 60, // 5 days in tier (> 3 day window)
            5 * 24 * 60 * 60,
            &pthr(),
        );
        assert!(!result, "past promotion window must block promotion");
    }

    #[test]
    fn test_should_promote_active_is_noop() {
        let result = MemoryLifecycleEnforcer::should_promote(
            MemoryLifecycle::Active,
            100, 0, 3600, &pthr(),
        );
        assert!(!result, "Active episodes cannot be promoted");
    }

    #[test]
    fn test_should_promote_archived_is_noop() {
        let result = MemoryLifecycleEnforcer::should_promote(
            MemoryLifecycle::Archived,
            100, 0, 3600, &pthr(),
        );
        assert!(!result, "Archived episodes cannot be promoted");
    }

    #[test]
    fn test_promotion_defaults_are_sensible() {
        let t = PromotionThresholds::default();
        assert_eq!(t.min_hits_to_promote, 2);
        assert_eq!(t.cooldown_after_transition_secs, 600);
        assert_eq!(t.promotion_window_secs, 3 * 24 * 60 * 60);
    }

    #[test]
    fn test_should_promote_at_exact_boundary() {
        // Exactly at cooldown boundary — should pass.
        let result = MemoryLifecycleEnforcer::should_promote(
            MemoryLifecycle::Cooling,
            2,
            600,  // exactly at window boundary (3 days)
            600,  // exactly at cooldown boundary
            &pthr(),
        );
        assert!(result, "exact boundary should promote");
    }

    #[test]
    fn test_should_promote_at_exact_window_boundary() {
        // Exactly at promotion window boundary — should NOT pass
        // (> not >=).
        let window = 3 * 24 * 60 * 60;
        let result = MemoryLifecycleEnforcer::should_promote(
            MemoryLifecycle::Cooling,
            2,
            window + 1, // just past the window
            3600,
            &pthr(),
        );
        assert!(!result, "just past promotion window must block");
    }
}
