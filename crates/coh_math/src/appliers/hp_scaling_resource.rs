//! PASS2B-2 regen/recovery Expression reader — recovers the HP-scaling regen/recovery magnitude
//! atom-native by EVALUATING the `Regeneration`/`Recovery`, `aspect=Current`, `type=Expression`
//! atom's `magnitude_expression`, replacing the bag's flat `+100%` placeholder (the scale×table
//! the `resource_buff_value` PUNT falls back to). The canonical case is **Gamma Boost**
//! (Radiation Armor), whose regen and recovery scale with the caster's current-HP%:
//!
//! - Regen — `75 kHitPoints% source> - 30 + 100 / @StdResult *` → `(105 − H)/100 × @StdResult`,
//!   rising as current-HP% (`H`) falls ("the lower your current health, the greater the regen").
//! - Recovery — `1.2 kHitPoints% source> * 100 / .3 * @StdResult *` → `1.2·H/100 × 0.3 × @StdResult`,
//!   rising with current-HP% ("the higher your current health, the greater the recovery").
//!
//! Unlike ATOM10's absorb fraction (whose `Max.kHitPoints` is build-time-known and neutralized to
//! unity), `kHitPoints%` is genuine runtime state the user supplies via `combat.hit_points_percent`
//! (default full health). `@StdResult` = the atom's own scale×table — resolved here, not
//! neutralized, because the expression multiplies it in to reach the final magnitude. The result is
//! the COMPLETE magnitude (already × @StdResult), so the caller adds it directly and must NOT re-run
//! `resolve_scaled_effect` on it.
//!
//! The same input drives Super Reflexes' scaling resistance ([`hp_scaling_resistance_value`]):
//! Agile, Dodge and Lucky state `60 kHitPoints% source> - 0 100 minmax 60 / 0.2 * [@Strength *]`,
//! nothing at 60% health and above, rising linearly to the coefficient at zero. Its `scale` is 0 on
//! `Melee_Ones`, so the ordinary resistance reader resolves it to a silent zero.

use super::base_atoms_of_type;
use super::resistance::RESIST_STD_SUBTYPES;
use crate::expr::{eval, EvalContext, EvalError, Value};
use crate::scaled::resolve_scaled_effect;
use crate::totals::CalcError;
use coh_data::{
    reaches_caster, Aspect, AtomicEffect, AttribType, CombatContext, EffectType, Power,
    PowerDatabase, PvMode,
};

/// Resolves the two readers an HP-scaling resource Expression names: `kHitPoints% source>` (the
/// caster's current-HP%, the runtime input) and `@StdResult` (the atom's own scale×table, the base
/// magnitude the expression modulates). `@Strength` resolves only where the caller supplies the
/// power's enhancement strength (the resistance reader). Every other reader is Indeterminate, so
/// only this shape yields a number — an unrecognized reader falls back to the bag rather than a
/// fabricated value (Rule 1).
struct HitPointsProbe {
    hit_points_percent: f64,
    std_result: f64,
    strength: Option<f64>,
}

impl EvalContext for HitPointsProbe {
    fn resolve(&self, reader: &str, operands: &[Value]) -> Result<Value, EvalError> {
        match reader {
            "source>" | "Source>" => match operands.first() {
                Some(Value::Symbol(s)) if &**s == "kHitPoints%" => {
                    Ok(Value::Number(self.hit_points_percent))
                }
                _ => Err(EvalError::Indeterminate(reader.into())),
            },
            "@StdResult" => Ok(Value::Number(self.std_result)),
            "@Strength" => self
                .strength
                .map(Value::Number)
                .ok_or_else(|| EvalError::Indeterminate(reader.into())),
            _ => Err(EvalError::Indeterminate(reader.into())),
        }
    }
}

/// What the HP-scaling Expression contributes to one resource half.
#[derive(Debug, Clone, PartialEq)]
pub struct HpScalingResource {
    /// The FINAL magnitude — already × `@StdResult`, so the caller adds it directly and must
    /// never re-run `resolve_scaled_effect` on it.
    pub value: f64,
    /// The contributing atoms' `modifierTable`, for the caller's `Res_Boolean` guard. Carried
    /// because the Expression now admits itself (ATOM-BAG-5): with no bag slot behind it there
    /// is no other table for that guard to read.
    pub table: Option<String>,
}

/// The HP-scaling regen or recovery magnitude read atom-native, or `None` when this power has no
/// such atom (the bag placeholder then supplies the half, if it carries one).
///
/// Matches the atoms `resource_buff_value` PUNTs: BASE atoms of `effect_type`, `aspect=Current`,
/// `type=Expression`, self-directed (`toWho` Self/All), not caster-excluded. Sums all such atoms
/// (Gamma Boost carries exactly one per effect type). `None` when there is no such atom, its
/// `scale`/`magnitude_expression` is missing, or the expression falls outside the resolvable shape
/// (any Indeterminate reader) — which is how the Kheldian `endurancecost power.boosted>` toggles
/// and Fortify Pack's `Cur.kMeter` stay out of this reader entirely.
pub fn hp_scaling_resource_value(
    power: &Power,
    effect_type: EffectType,
    archetype: &str,
    level: i32,
    combat: &CombatContext,
    db: &PowerDatabase,
    errors: &mut Vec<CalcError>,
) -> Option<HpScalingResource> {
    let mut total: Option<f64> = None;
    let mut table: Option<String> = None;
    for atom in base_atoms_of_type(power, effect_type) {
        if atom.aspect != Some(Aspect::Cur)
            || atom.attrib_type != Some(AttribType::Expression)
            || atom.not_on_caster == Some(true)
            || !reaches_caster(atom, power)
        {
            continue;
        }
        let std_result = resolve_scaled_effect(
            atom.scale?,
            atom.modifier_table.as_deref(),
            archetype,
            level,
            db,
            errors,
        );
        let probe = HitPointsProbe {
            hit_points_percent: combat.hit_points_percent,
            std_result,
            strength: None,
        };
        let expression = atom
            .magnitude_expression
            .as_deref()
            .filter(|e| !e.is_empty())?;
        let value = match eval(expression, &probe) {
            Ok(Value::Number(n)) => n,
            _ => return None,
        };
        total = Some(total.unwrap_or(0.0) + value);
        table = atom.modifier_table.as_deref().map(str::to_string);
    }
    total.map(|value| HpScalingResource { value, table })
}

/// The HP-scaling +resistance this power grants the caster at `combat.hit_points_percent`, as
/// FRACTIONS keyed by lowercase damage type. Empty when the power has no such atom.
///
/// Matches BASE `Resistance` atoms with `aspect=Res`, `type=Expression`, a standard damage type, a
/// `kHitPoints%` term, and the caster as recipient. A PvP-only twin (Agile's Toxic row) counts only
/// on a PvP map, and a PvE-only one only off it. `strength` is the power's resistance enhancement
/// (`1 + enh`), and reaches the value only through an `@Strength` the expression names: Rebirth
/// writes it in, Homecoming does not, and the engine applies no strength to an Expression
/// magnitude it is not told to. Atoms of one type are copies of one effect, so the largest stands.
pub fn hp_scaling_resistance_value(
    power: &Power,
    archetype: &str,
    level: i32,
    combat: &CombatContext,
    strength: f64,
    db: &PowerDatabase,
    errors: &mut Vec<CalcError>,
) -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = Vec::new();
    for atom in base_atoms_of_type(power, EffectType::Resistance) {
        let Some(sub) = atom.sub_type.filter(|s| RESIST_STD_SUBTYPES.contains(s)) else {
            continue;
        };
        if atom.aspect != Some(Aspect::Res)
            || atom.attrib_type != Some(AttribType::Expression)
            || atom.not_on_caster == Some(true)
            || !reaches_caster(atom, power)
            || !pv_mode_applies(atom, combat.pvp)
        {
            continue;
        }
        let Some(expression) = atom
            .magnitude_expression
            .as_deref()
            .filter(|e| e.iter().any(|t| &**t == "kHitPoints%"))
        else {
            continue;
        };
        let std_result = resolve_scaled_effect(
            atom.scale.unwrap_or(0.0),
            atom.modifier_table.as_deref(),
            archetype,
            level,
            db,
            errors,
        );
        let probe = HitPointsProbe {
            hit_points_percent: combat.hit_points_percent,
            std_result,
            strength: Some(strength),
        };
        let Ok(Value::Number(value)) = eval(expression, &probe) else {
            continue;
        };
        let key = sub.as_wire().to_lowercase();
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => *v = v.max(value),
            None => out.push((key, value)),
        }
    }
    out
}

fn pv_mode_applies(atom: &AtomicEffect, pvp: bool) -> bool {
    match atom.pv_mode {
        Some(PvMode::PvP) => pvp,
        Some(PvMode::PvE) => !pvp,
        _ => true,
    }
}
