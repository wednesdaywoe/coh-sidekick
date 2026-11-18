//! Henchmen — the pets ONE summon power fields, shown in the Info panel below the summon
//! power's own card as its damage section (owner ruling: the card keeps its information and
//! gains the pets' damage readout, the way any damage power carries one): the roster's damage
//! headline and, per pet, survivability,
//! the upgrade tier that swaps its attack set, and every attack's damage. Reads
//! [`coh_math::henchmen`]'s read-only per-power projection and nothing else; the view computes
//! no stat of its own.
//!
//! The headline is the power's own, from one projection memo over the build's held upgrades.
//! The drill-in re-runs `pet_detail` at the tier the reader chooses and may show the build's
//! pet buffs on top — both are views, neither writes the build, and neither moves the
//! headline: the buff is opt-in per pet, exactly as the engine keeps it.

use crate::build_session::BuildSession;
use crate::shell::Db;
use coh_data::{CharacterState, Power};
use coh_math::granted::mastermind_henchman;
use coh_math::henchmen::{self, BuffDelta, DAMAGE_BASIS};
use coh_math::totals::CalcError;
use dioxus::prelude::*;

/// The Info panel's henchman face: the damage headline over the roster one summon power
/// fields, and the drill-in a roster row opens.
#[component]
pub fn HenchmanView(database: Db, power: Power) -> Element {
    let session = use_context::<BuildSession>();
    let mut selected = use_signal(|| Option::<String>::None);

    // `holds` is the build's own picked powers: an upgrade granter is held exactly when the
    // build picked it, which is what the engine's tier walk asks.
    let projection = use_memo(use_reactive!(|database, power| {
        let build = session.build.read();
        henchmen::project_power(&database, &power, build.level, &|name| {
            build.picked_powers().any(|pick| pick.internal_name == name)
        })
    }));

    // The meter's scale: the same roster with no upgrades (the pale anchor fill) and with
    // every tier held (the track's end).
    let scale = use_memo(use_reactive!(|database, power| {
        let build = session.build.read();
        let base = henchmen::project_power(&database, &power, build.level, &|_| false).headline_dps;
        let full = henchmen::project_power(&database, &power, build.level, &|_| true).headline_dps;
        (base, full)
    }));

    let (roster, pets, headline, errors) = {
        let proj = projection.read();
        (
            proj.roster.clone(),
            proj.pets.clone(),
            proj.headline_dps,
            proj.errors.clone(),
        )
    };
    let level = session.build.read().level;
    let (base_dps, full_dps) = scale();
    let reference = full_dps.max(headline).max(base_dps);
    let width = |value: f64| (value / reference * 100.0).clamp(0.0, 100.0);
    let total_width = width(headline);
    let base_width = width(base_dps);
    let scale_note = if full_dps > 0.0 && headline >= full_dps {
        "full bar — every upgrade tier this roster can hold is held"
    } else {
        "scaled to the same roster with every upgrade tier held; the pale fill is the roster with no upgrades"
    };

    let rows: Vec<(String, f64, String, f64)> = roster
        .iter()
        .map(|entry| {
            let pet = pets.iter().find(|pet| pet.entity == entry.entity);
            (
                entry.entity.clone(),
                entry.count,
                pet.map(|pet| pet.best_attack.clone()).unwrap_or_default(),
                pet.map(|pet| pet.best_proxy).unwrap_or(0.0),
            )
        })
        .collect();
    let total_count: f64 = roster.iter().map(|entry| entry.count).sum();
    // A selection the roster no longer has (the level dropped a gate) falls back to the
    // roster rather than writing the signal back — the same derived fallback Info's tabs use.
    let open = selected()
        .filter(|entity| roster.iter().any(|entry| &entry.entity == entity))
        .and_then(|entity| {
            let count = roster.iter().find(|entry| entry.entity == entity)?.count;
            Some((entity, count))
        });

    rsx! {
        div { class: "henchmen",
            if roster.is_empty() {
                p { class: "henchmen-empty",
                    "No henchmen fielded — this power summons no Mastermind henchmen at the build's current level."
                }
            } else {
                div { class: "info-block",
                    div { class: "info-block__head",
                        h4 { "Henchmen" }
                    }
                div { class: "damage-hero",
                    span { class: "damage-hero__value", "{headline:.2}" }
                    span { class: "damage-hero__base",
                        "dmg/s · roster of "
                        span { class: "damage-hero__base-value", "{count_text(total_count)}" }
                    }
                }
                if reference > 0.0 {
                    div {
                        class: "damage-bar",
                        role: "img",
                        title: "{scale_note}",
                        "aria-label": "Henchman damage relative to the same roster fully upgraded",
                        div { class: "damage-bar__fill damage-bar__fill--total", style: "width: {total_width}%;" }
                        div { class: "damage-bar__fill damage-bar__fill--base", style: "width: {base_width}%;" }
                    }
                }
                p { class: "henchmen-basis",
                    "{DAMAGE_BASIS} · L{level} pet-class tables (henchman level offset unmodeled) · no transient buffs"
                }
                for (index, err) in errors.iter().enumerate() {
                    div { key: "{index}", class: "load-state error", "{err.context}: {err.detail}" }
                }
                div { class: "info-lines",
                    for (entity, count, best_attack, best_proxy) in rows {
                        {
                            let is_open = open.as_ref().is_some_and(|(open_entity, _)| open_entity == &entity);
                            let label = entity_label(&entity);
                            let contribution = best_proxy * count;
                            rsx! {
                                div {
                                    key: "{entity}",
                                    class: "stat-row",
                                    class: if is_open { "is-selected" },
                                    role: "button",
                                    tabindex: "0",
                                    aria_pressed: "{is_open}",
                                    title: "Open {label}'s own stats",
                                    onclick: {
                                        let entity = entity.clone();
                                        move |_| selected.set(Some(entity.clone()))
                                    },
                                    onkeydown: {
                                        let entity = entity.clone();
                                        move |evt: KeyboardEvent| {
                                            if matches!(evt.key(), Key::Enter) || evt.key() == Key::Character(" ".into()) {
                                                evt.prevent_default();
                                                selected.set(Some(entity.clone()));
                                            }
                                        }
                                    },
                                    span { class: "stat-label", "{label} ×{count_text(count)}" }
                                    span { class: "leader" }
                                    span {
                                        class: "stat-value mono",
                                        style: "--stat-hue: var(--stat-damage);",
                                        "{contribution:.2}/s"
                                    }
                                    if !best_attack.is_empty() {
                                        span { class: "henchmen-roster__note",
                                            "{best_attack} · {best_proxy:.2}/s each"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                }
                if let Some((entity, count)) = open {
                    PetDrillIn {
                        key: "{entity}",
                        database: database.clone(),
                        entity,
                        count,
                        on_close: move |_| selected.set(None),
                    }
                }
            }
        }
    }
}

/// One upgrade tier as the selector offers it: the granter powers that switch it on,
/// cumulative from the base — a tier SWAPS the ability set (the engine revokes what it
/// replaces), so "+Tactical" means Equip held too, never the two added together.
#[derive(Clone)]
struct UpgradeChoice {
    label: String,
    granters: Vec<String>,
}

/// The tiers this entity's export states, in tier order, each choice holding every granter
/// up to it. Base is always choice 0; an entity with no tiers offers Base alone.
fn upgrade_choices(db: &Db, entity: &str) -> Vec<UpgradeChoice> {
    let mut tiers: Vec<(i64, Vec<String>)> = mastermind_henchman(db, entity)
        .and_then(|entity| entity.get("upgradeTiers"))
        .and_then(|tiers| tiers.as_array())
        .map(|tiers| {
            tiers
                .iter()
                .map(|tier| {
                    (
                        tier.get("tier").and_then(|tier| tier.as_i64()).unwrap_or(0),
                        tier.get("grantedBy")
                            .and_then(|granters| granters.as_array())
                            .map(|granters| {
                                granters
                                    .iter()
                                    .filter_map(|name| name.as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    tiers.sort_by_key(|(tier, _)| *tier);

    let mut choices = vec![UpgradeChoice {
        label: "Base".to_string(),
        granters: Vec::new(),
    }];
    let mut held: Vec<String> = Vec::new();
    for (_, granters) in tiers {
        let label = granters
            .first()
            .and_then(|name| db.all_powers().find(|power| power.ident() == name))
            .map(|power| format!("+{}", power.name))
            .unwrap_or_else(|| "+Upgrade".to_string());
        for granter in &granters {
            if !held.contains(granter) {
                held.push(granter.clone());
            }
        }
        choices.push(UpgradeChoice {
            label,
            granters: held.clone(),
        });
    }
    choices
}

/// The tier the build itself holds: the longest prefix of choices whose granters it picked.
/// The selector opens here, so the drill-in agrees with the headline until the reader moves it.
fn build_choice(build: &CharacterState, choices: &[UpgradeChoice]) -> usize {
    let mut choice = 0;
    for (index, tier) in choices.iter().enumerate().skip(1) {
        let held = tier
            .granters
            .iter()
            .any(|granter| build.picked_powers().any(|pick| &pick.internal_name == granter));
        if held {
            choice = index;
        } else {
            break;
        }
    }
    choice
}

/// One attack as the table draws it: adjacent equal components grouped into "n × type" chips.
struct AttackView {
    name: String,
    hits: Vec<(String, f64, u32)>,
    per_activation: f64,
    recharge: f64,
    cast: f64,
    proxy: f64,
    is_best: bool,
    moved: bool,
}

/// The pet detail a roster row opens: survivability, the upgrade selector, the attack table,
/// and the opt-in buff view. Keyed by entity in the roster, so switching pets remounts it and
/// the tier/buff choices start from the build again. `count` is the roster's own count for
/// this entity, passed down so the drill-in reads the same roster the headline drew.
#[component]
fn PetDrillIn(database: Db, entity: String, count: f64, on_close: EventHandler<()>) -> Element {
    let session = use_context::<BuildSession>();
    let mut chosen = use_signal(|| Option::<usize>::None);
    let mut buff_on = use_signal(|| false);

    let choices = upgrade_choices(&database, &entity);
    let memo_choices = choices.clone();
    let detail = use_memo(use_reactive!(|database, entity, count| {
        let build = session.build.read();
        let level = i32::from(build.level);
        let choice = chosen().unwrap_or_else(|| build_choice(&build, &memo_choices));
        let buffed = buff_on();
        let mut errors: Vec<CalcError> = Vec::new();
        let granters = memo_choices
            .get(choice)
            .map(|tier| tier.granters.clone())
            .unwrap_or_default();
        let holds = |name: &str| granters.iter().any(|granter| granter == name);
        let base = henchmen::pet_detail(&database, &entity, level, count, &holds, &mut errors)?;
        let (shown, delta) = if buffed {
            let delta = henchmen::buff_delta(&database, &build, &base.class, level, &mut errors);
            (henchmen::buffed_pet(&base, &delta), delta)
        } else {
            (base.clone(), BuffDelta::default())
        };
        Some((base, shown, delta, errors))
    }));

    let Some((base, shown, delta, errors)) = detail() else {
        return rsx! {
            div { class: "henchman-pet",
                div { class: "henchman-pet__head",
                    button {
                        class: "seg",
                        r#type: "button",
                        onclick: move |_| on_close.call(()),
                        "← Roster"
                    }
                }
                p { class: "henchmen-empty",
                    "This henchman is not fielded at the build's current level."
                }
            }
        };
    };

    let choice = chosen().unwrap_or_else(|| build_choice(&session.build.read(), &choices));
    let buffed = buff_on();
    let label = entity_label(&entity);
    let count = shown.count;
    let class = shown.class.clone();
    let tabs: Vec<(String, String)> = choices
        .iter()
        .map(|tier| {
            let title = if tier.granters.is_empty() {
                "No upgrade powers held".to_string()
            } else {
                format!("Held granters: {}", tier.granters.join(", "))
            };
            (tier.label.clone(), title)
        })
        .collect();

    let mut survivability: Vec<(String, String, &'static str, bool)> = Vec::new();
    for (ty, value) in &shown.survivability.resistance {
        let moved = buffed && delta.resistance.get(ty).copied().unwrap_or(0.0) > 0.0;
        survivability.push((
            format!("{} resistance", type_label(ty)),
            format!("{:.1}%", value * 100.0),
            "var(--stat-resistance)",
            moved,
        ));
    }
    for (ty, value) in &shown.survivability.defense {
        survivability.push((
            format!("{} defense", type_label(ty)),
            format!("{:.1}%", value * 100.0),
            "var(--stat-defense)",
            false,
        ));
    }
    for (ty, value) in &shown.survivability.mez_protection {
        survivability.push((
            format!("{} protection", type_label(ty)),
            format!("mag {value:.0}"),
            "var(--stat-mez)",
            false,
        ));
    }
    for (ty, value) in &shown.survivability.mez_resistance {
        survivability.push((
            format!("{} mez resistance", type_label(ty)),
            format!("{:.0}%", value * 100.0),
            "var(--stat-mez)",
            false,
        ));
    }

    let attacks: Vec<AttackView> = shown
        .attacks
        .iter()
        .map(|attack| {
            let mut hits: Vec<(String, f64, u32)> = Vec::new();
            for component in &attack.components {
                if let Some(last) = hits.last_mut() {
                    if last.0 == component.damage_type
                        && (last.1 - component.per_hit).abs() < 1e-9
                    {
                        last.2 += 1;
                        continue;
                    }
                }
                hits.push((component.damage_type.clone(), component.per_hit, 1));
            }
            let moved = buffed
                && base
                    .attacks
                    .iter()
                    .find(|base_attack| base_attack.internal_name == attack.internal_name)
                    .is_some_and(|base_attack| {
                        (base_attack.proxy_dps - attack.proxy_dps).abs() > 1e-9
                    });
            AttackView {
                name: attack.display_name.clone(),
                hits,
                per_activation: attack.damage_per_activation,
                recharge: attack.recharge,
                cast: attack.cast_time,
                proxy: attack.proxy_dps,
                is_best: attack.display_name == shown.best_attack,
                moved,
            }
        })
        .collect();

    let mut buff_rows: Vec<(String, String)> = Vec::new();
    if buffed {
        for (ty, value) in &delta.damage {
            if *value > 0.0 {
                buff_rows.push((
                    format!("Damage · {}", type_label(ty)),
                    format!("+{:.0}%", value * 100.0),
                ));
            }
        }
        for (ty, value) in &delta.resistance {
            if *value > 0.0 {
                buff_rows.push((
                    format!("Resistance · {}", type_label(ty)),
                    format!("+{:.1}%", value * 100.0),
                ));
            }
        }
        if delta.to_hit > 0.0 {
            buff_rows.push(("To-hit".to_string(), format!("+{:.1}%", delta.to_hit * 100.0)));
        }
        if delta.recovery > 0.0 {
            buff_rows.push((
                "Recovery".to_string(),
                format!("+{:.1}%", delta.recovery * 100.0),
            ));
        }
    }

    rsx! {
        div { class: "henchman-pet",
            div { class: "henchman-pet__head",
                button {
                    class: "seg",
                    r#type: "button",
                    title: "Back to the roster",
                    onclick: move |_| on_close.call(()),
                    "← Roster"
                }
                span { class: "henchman-pet__name", "{label}" }
                span { class: "henchman-pet__count", "×{count_text(count)} fielded" }
                span { class: "henchman-pet__class mono", "{class}" }
            }
            nav { class: "info-tabs", role: "tablist", "aria-label": "Upgrade tier",
                for (index, (label, title)) in tabs.iter().enumerate() {
                    button {
                        key: "{index}",
                        class: if index == choice { "info-tab is-active" } else { "info-tab" },
                        r#type: "button",
                        role: "tab",
                        "aria-selected": index == choice,
                        title: "{title}",
                        onclick: move |_| chosen.set(Some(index)),
                        "{label}"
                    }
                }
            }
            label { class: "info-rank",
                input {
                    r#type: "checkbox",
                    checked: buffed,
                    onchange: move |evt: Event<FormData>| buff_on.set(evt.checked()),
                }
                span { "Show the build's pet buffs — opt-in, the roster headline does not change" }
            }
            for (index, err) in errors.iter().enumerate() {
                div { key: "{index}", class: "load-state error", "{err.context}: {err.detail}" }
            }
            div { class: "info-block",
                h4 { "Survivability" }
                if survivability.is_empty() {
                    p { class: "henchmen-empty", "This henchman's abilities grant no defensive stats." }
                } else {
                    div { class: "info-lines",
                        for (index, (label, value, hue, moved)) in survivability.into_iter().enumerate() {
                            div { key: "{index}", class: "info-line",
                                span { class: "info-line__label", "{label}" }
                                span { class: "leader" }
                                span {
                                    class: "info-line__value",
                                    class: if moved { "is-buffed" },
                                    style: "--stat-hue: {hue};",
                                    "{value}"
                                }
                            }
                        }
                    }
                }
            }
            div { class: "info-block",
                div { class: "info-block__head",
                    h4 { "Attacks" }
                    span { class: "henchmen-basis", "{DAMAGE_BASIS}" }
                }
                if attacks.is_empty() {
                    p { class: "henchmen-empty", "This henchman deals no damage at this tier." }
                } else {
                    div { class: "info-rows",
                        div { class: "info-rows__head",
                            span { "Attack" }
                            span { "Per act." }
                            span { "Rech." }
                            span { "dmg/s" }
                        }
                        for (index, attack) in attacks.into_iter().enumerate() {
                            div { key: "{index}", class: "info-row",
                                span { class: "info-row__label",
                                    "{attack.name}"
                                    if attack.is_best {
                                        span { class: "info-flag", "best" }
                                    }
                                    for (index, (ty, per_hit, hits)) in attack.hits.iter().enumerate() {
                                        span {
                                            key: "{index}",
                                            class: "damage-chip",
                                            "data-damage": "{ty}",
                                            if *hits > 1 { "{hits}× " }
                                            "{ty} {per_hit:.2}"
                                        }
                                    }
                                }
                                span { class: "info-row__base", "{attack.per_activation:.2}" }
                                span {
                                    class: "info-row__tier",
                                    title: "cast {attack.cast:.2}s",
                                    "{attack.recharge:.1}s"
                                }
                                span {
                                    class: "info-row__total",
                                    class: if attack.moved { "is-buffed" },
                                    "{attack.proxy:.2}"
                                }
                            }
                        }
                    }
                }
            }
            if buffed {
                div { class: "info-block",
                    h4 { "Pet buff applied" }
                    if buff_rows.is_empty() {
                        p { class: "henchmen-empty",
                            "No active pet buff in this build reaches this henchman."
                        }
                    } else {
                        dl { class: "info-pairs",
                            for (index, (label, value)) in buff_rows.into_iter().enumerate() {
                                dt { key: "{index}", "{label}" }
                                dd { class: "is-buffed", "{value}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The name a roster row wears: the entity's own suffix, spaced — `MastermindPets_Spec_Ops`
/// reads as "Spec Ops", which is what the summon power calls it.
fn entity_label(entity: &str) -> String {
    entity
        .strip_prefix("MastermindPets_")
        .unwrap_or(entity)
        .replace('_', " ")
}

/// A count as the roster states it: whole where it is whole (the engine sums magnitudes).
fn count_text(count: f64) -> String {
    if count.fract() == 0.0 {
        format!("{count:.0}")
    } else {
        format!("{count:.1}")
    }
}

/// A survivability map's lowercase type key as a label. The empty key is an untyped grant —
/// it reaches every type, and says so rather than drawing a blank label.
fn type_label(ty: &str) -> String {
    let mut chars = ty.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "All types".to_string(),
    }
}
