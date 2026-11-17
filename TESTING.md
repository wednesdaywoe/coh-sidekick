# TESTING

Built items awaiting the owner's own pass. Each says what to do and what done looks like;
the worklog item it answers is named. Agent-runnable checks are already ticked there.

## Mastermind totals (desktop or web)

- [ ] Open a level-50 Mercenaries Mastermind build on Homecoming with Serum picked and
      active (worklog `pet-only-buffs-3`). Done: the dashboard's damage / to-hit / recovery /
      resistance / mez-protection totals are the same with Serum active as without it, and the
      Serum card still lists its +DMG / +RES / +To Hit / +Recovery effects. Repeat on Brainstorm
      (same leak); Rebirth and Thunderspy were already clean. On a Ninjas build, Smoke Flash must
      not add stealth radius to the MM either.

## Sign-in (web, next deployment)

- [ ] After deploy, sign in with SimpleLogin on next.coh-sidekick.com
      (worklog `oauth-state-retry-3`). Done: signed in on the first attempt or after one
      silent restart, with no "That didn't work" modal. If a modal appears, note the red
      code line — anything other than `oauth_client_state_not_found` is a different bug.
