# TESTING

Built items awaiting the owner's own pass. Each says what to do and what done looks like;
the worklog item it answers is named. Agent-runnable checks are already ticked there.

## Sign-in (web, next deployment)

- [ ] After deploy, sign in with SimpleLogin on next.coh-sidekick.com
      (worklog `oauth-state-retry-3`). Done: signed in on the first attempt or after one
      silent restart, with no "That didn't work" modal. If a modal appears, note the red
      code line — anything other than `oauth_client_state_not_found` is a different bug.
