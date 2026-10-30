//! How far into Enhancement Diversification each aspect of one power's slotting is: what is
//! slotted, what counts, which stage of diminishing returns it has reached, and how much ED
//! takes.
//!
//! "Counts" is [`power_enhancement`]'s answer, the per-power enhancement the totals and the power
//! projection already use, Alpha's ED split included. So the breakdown can never disagree with
//! the totals about a power.

use crate::apply::power_enhancement;
use crate::enhancement::{
    apply_ed, filter_alpha_by_allowed_enhancements, raw_power_enhancement_bonuses,
    schedule_for_aspect,
};
use crate::incarnates::AlphaEnhancement;
use coh_data::{CombatContext, Enhancement, Level, Power, PowerDatabase};

/// One aspect of a power's enhancement, through Enhancement Diversification. Values are
/// fractions (0.95 is +95%).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AspectBreakdown {
    /// The normalized aspect key (`"damage"`, `"recharge"`).
    pub aspect: &'static str,
    /// What the slotted enhancements add up to before ED. Alpha is not in it.
    pub slotted: f64,
    /// Alpha's share that ED cuts, which joins `slotted` before the ED pass. Zero without one.
    pub alpha: f64,
    /// What counts: after ED, plus Alpha's share that bypasses it. The power's enhancement.
    pub counted: f64,
    /// The ED stage `slotted + alpha` reached: 0 counts in full, then 1, 2 and 3 for each
    /// threshold passed, where each further point counts for less.
    pub stage: u8,
    /// What ED cuts from `slotted + alpha`: the diminishing returns, as a fraction. Zero at
    /// stage 0.
    pub lost: f64,
    /// The aspect's ED thresholds, so a surface can draw where the total sits.
    pub thresholds: [f64; 3],
}

/// Each aspect the power's slotting enhances, in [`power_enhancement`]'s terms. Empty for a
/// power with nothing slotted, or a dataset without enhancement curves.
pub fn power_enhancement_breakdown(
    slots: &[Option<Enhancement>],
    power: &Power,
    alpha: &AlphaEnhancement,
    level: i32,
    combat: &CombatContext,
    db: &PowerDatabase,
) -> Vec<AspectBreakdown> {
    let (Some(curves), Some(io_level)) = (
        db.enhancement_curves.as_ref(),
        Level::from_i64(i64::from(level)),
    ) else {
        return Vec::new();
    };
    if slots.iter().all(Option::is_none) {
        return Vec::new();
    }
    // Faults here are the ones the totals already report for the same power.
    let mut quiet = Vec::new();
    let raw = raw_power_enhancement_bonuses(
        slots,
        io_level,
        db.io_sets.as_ref(),
        combat.exemplar_level,
        curves,
        &mut quiet,
    );
    let counted = power_enhancement(slots, power, alpha, level, combat, db, &mut quiet);
    let allowed = power.allowed_enhancements.as_deref();
    let (alpha_total, alpha_bypass) = if alpha.active {
        (
            filter_alpha_by_allowed_enhancements(&alpha.bonuses, allowed),
            filter_alpha_by_allowed_enhancements(&alpha.ed_bypass, allowed),
        )
    } else {
        Default::default()
    };
    raw.iter()
        .filter_map(|(aspect, slotted)| {
            let schedule = schedule_for_aspect(aspect, curves)?;
            let thresholds = curves.schedules.get(schedule).ed_thresholds;
            let alpha_share = alpha_total.get(aspect) - alpha_bypass.get(aspect);
            let total = slotted + alpha_share;
            let stage = thresholds.iter().filter(|&&at| total > at).count() as u8;
            let lost = (total - apply_ed(total, schedule, curves)).max(0.0);
            Some(AspectBreakdown {
                aspect,
                slotted,
                alpha: alpha_share,
                counted: counted.get(aspect),
                stage,
                lost,
                thresholds,
            })
        })
        .collect()
}
