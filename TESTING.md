# TESTING

Built items awaiting the owner's own pass. Each says what to do and what done looks like;
the worklog item it answers is named. Agent-runnable checks are already ticked there.

## Mastermind totals (desktop or web)

- [x] Open a level-50 Mercenaries Mastermind build on Homecoming with Serum picked and
      active (worklog `pet-only-buffs-3`). Done: the dashboard's damage / to-hit / recovery /
      resistance / mez-protection totals are the same with Serum active as without it, and the
      Serum card still lists its +DMG / +RES / +To Hit / +Recovery effects. Repeat on Brainstorm
      (same leak); Rebirth and Thunderspy were already clean. On a Ninjas build, Smoke Flash must
      not add stealth radius to the MM either. Owner confirmed 2026-11-17: Serum no longer
      buffs the player.

## Henchmen in the Info panel (desktop or web)

- [ ] Open a level-50 Mercenaries Mastermind build on Homecoming with Soldiers, Spec Ops,
      Commando, Equip Mercenary and Tactical Upgrade picked (worklog `henchman-panel-2`).
      Done: pointing at Soldiers shows its henchmen IN the Info panel, in place of the generic
      power card — a damage headline meter in the dashboard's damage hue reading ≈6.75 dmg/s
      over Soldier ×2 + Medic ×1 (the Medic counts, it fires SMG_Burst), with the basis label
      "best-attack spam proxy · L50 pet-class tables". Spec Ops reads its own roster (≈12.38)
      and Commando its own (≈4.13); every other power still reads its normal card. Clicking a
      roster row opens that pet's detail. Judge the meter scale and the readout's fit inside
      the Info panel.
- [ ] In the pet detail (worklog `henchman-panel-3`). Done: survivability is stat-colored
      (Soldier ≈26.1% smashing/lethal resistance); an upgrade selector (Base / +Equip /
      +Tactical) SWAPS the attack table rather than adding to it; the attack table lists each
      attack's damage, recharge and dmg/s proxy (Quick Burst 6.74 over 3s → 2.25/s); and the
      "show pet buffs" toggle reveals the buffed state WITHOUT moving the power's headline.

## Sign-in (web, next deployment)

- [ ] After deploy, sign in with SimpleLogin on next.coh-sidekick.com
      (worklog `oauth-state-retry-3`). Done: signed in on the first attempt or after one
      silent restart, with no "That didn't work" modal. If a modal appears, note the red
      code line — anything other than `oauth_client_state_not_found` is a different bug.
