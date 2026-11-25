# Letting two or more people edit one shared build

Status: **proposal, nothing built.** Written 2026-11-25 in response to a user request for
"Google Docs for builds — several people working on one character at the same link," and revised
the same day after the requesting use case came into focus: **two people on a call, making a
collaboration video, tuning one build together.** That is a live session, and it reorders the
design: presence and turn-taking are the headline feature, and the change-request model slides
to a later phase for the asynchronous case.

This doc states what already exists (with file and line references), the model it recommends,
what it rejects and why, and the staging. The enforcement half lands in the Supabase edge
functions, which do not live in this repo; the client half lands here.

## The ask, and the accurate version of the premise

The premise behind the ask — "the planner already live-edits, because the link keeps pointing at
the build as it changes" — is true in a specific sense: **Copy short link** (`quick_share`,
`crates/app/src/cloud/save_build.rs:405`) creates one `unlisted` row and then **updates that same
row in place** on every re-copy, which is what the fingerprint cache in
`crates/app/src/cloud/owner_store.rs` exists to make free. The URL never changes and always
resolves to the latest save (`{origin}/builds/{id}`, `browser.rs:copy_url`).

What exists is a shared, stable, re-writeable document with exactly one writer. What does not
exist is a second writer, a grant mechanism, or any merge — an update POSTs the whole skif v5
`build_json` envelope, so concurrent editing today is last-write-wins with silent loss. Nor does
live propagation: a second viewer only sees a new state if they reload.

## What the authorization model already is

A row is editable by whoever presents **either** of two credentials, client-side
(`save_build.rs:is_owned`, line 311) and server-side (`share-build`):

1. the **owner token** — a `crypto.randomUUID` minted at create, kept only in the creator's
   browser (`coh-planner-owner-tokens`), stored by the server as a SHA-256; or
2. a **session whose `auth.uid()` equals the row's `user_id`** (rows created signed-in), proven by
   the JWT the invoke already carries.

The split of powers matters for everything below:

| Act | Token holder | Owner account | Anyone else |
|---|---|---|---|
| Update content (`share-build` update) | ✅ | ✅ | ❌ 403 |
| Delete (`delete-build`) | ✅ | ✅ | ❌ |
| Change visibility | ❌ refused outright (`update-build-visibility/index.ts:88`) | ✅ | ❌ |
| Read a `private` row | ✅ (via `get-build`) | ✅ | ❌ |
| Read `unlisted` / `public` | ✅ | ✅ | ✅ (link / listed) |

And the machinery this feature reuses already exists too: `claim-builds` folds token-owned rows
into a signed-in account (`shared_builds.rs:claim_builds`, server cap 50/call,
`claim-builds/index.ts:68`), profiles carry `author_handle` (the browser view joins it, and the
author-profile feature reads it), the vault reads `user_id = eq.<uid>`
(`shared_builds.rs:my_builds_wire`, line 469), and Supabase Realtime is part of the same backend
product — nothing has used it yet, but it needs no new vendor.

## The model: one build, one pen

The session that motivated this feature does not need simultaneous editing. Two people tuning a
build on a call take turns at the keyboard *verbally already* — "no, try Defense instead of
Tank," "let me buff Hasten first, watch." So the model makes that rhythm a first-class object:

- The row's link is the room. Anyone the owner has granted joins it.
- Exactly one participant holds **the pen** and edits. The pen-holder's changes sync out; every
  other participant is a live viewer whose planner tracks the row, totals recalculating as the
  pen moves.
- The pen is handed over ("here, try it") or asked for. Presence lapsing — tab closed, connection
  dropped — releases it automatically.

The design consequence worth stating plainly: **a viewer holds no divergent state, so merge
conflicts are structurally impossible, not merely handled.** Whoever lacks the pen has no edits
to reconcile. This is what makes the feature affordable: no CRDT, no operational transform, no
per-field merge — the entire conflict story is one mutex.

And the rule the whole model stands on: **in a session, nobody edits without the pen — not even
the owner.** Viewer-mode local edits would be overwritten by the next live apply, so the client
blocks editing while watching and says "take the pen to edit" instead. One rule for everyone is
what keeps clobbering impossible; an owner-direct-write exception would reintroduce exactly the
silent loss this design exists to prevent.

### Rejected: sharing the owner token

An invite URL carrying the token in its fragment (`/builds/<id>#token=…`, fragments never reach
the server) would need **zero server changes** — the server already accepts a token for updates —
and was seriously considered, because the implementation is almost free. It is rejected on three
grounds:

- **The token is also the delete credential** (`delete_build`, `shared_builds.rs:1083`, accepts
  the token). A "can you tidy my enhancement?" invite would be a "you can delete my build" grant.
- **Unrevocable.** The server holds only the SHA-256; revocation means re-minting the secret and
  re-sharing every link. There is no way to answer "who can edit this?" at all.
- **Unattributable.** Every save by a token holder is indistinguishable from the owner's own.

If a coarse "anyone-with-the-link can edit" is ever wanted, it is a *separate* scoped secret with
its own column and no delete authority — not this token. Do not let convenience route the feature
back onto the owner token.

### Rejected for v1: free-for-all editing (every granted user writes)

The naive ACL — `token || owner || editor` all accept an update — was the first draft of this
doc. It is superseded: with whole-envelope last-write-wins, two granted writers editing the same
row need either a 409 ping-pong or a soft lock anyway, and a soft lock *is* the pen. So the
editor right is narrowed to **hold the pen, and (phase 2) propose** — never publish, delete, or
change visibility, never write the row except while holding the pen. The permission surface ends
up smaller than the naive ACL's, and `update-build-visibility`'s existing refusal of token
holders is the precedent it follows.

## Server design (Supabase repo; not this one)

### Grants: the `build_editors` table

```sql
create table public.build_editors (
  build_id       uuid not null references public.shared_builds(id) on delete cascade,
  editor_user_id uuid not null references auth.users(id)          on delete cascade,
  granted_by     uuid not null references auth.users(id)          on delete cascade,
  created_at     timestamptz not null default now(),
  primary key (build_id, editor_user_id)
);

alter table public.build_editors enable row level security;
-- No client policies: every access goes through the edge function under the service role,
-- which is the pattern every other write in this backend already follows.
```

Cascade on both sides is deliberate: deleting the build or the account must not strand grants,
for the same reason the delete-account flow exists.

### The pen: two columns and one atomic claim, in Postgres — not in Realtime

```sql
alter table public.shared_builds
  add column pen_holder  uuid references auth.users(id) on delete set null,
  add column pen_expires timestamptz,
  add column revision    bigint not null default 0;
```

One RPC, `hold-pen { buildId }`, does claim-or-renew atomically: it sets `pen_holder` to the
caller if the pen is free (`pen_holder is null or pen_expires < now()`) or already theirs, and
pushes `pen_expires` to `now() + interval '30 seconds'`; a held pen answers with the current
holder and its expiry. The client heartbeats every ~10 s while its tab is live. Handing over is
`release-pen`, callable by the holder or the owner; "ask for the pen" is presence signaling only
— the grant is the holder releasing.

Why Postgres rather than trusting Realtime presence state: Realtime is the *feel* (instant
banners, participant lists, "Sam asked for the pen"), but authority has to live where the write
path can check it in the same transaction context that performs it. `share-build` cannot verify
whoever Realtime *thinks* is holding the pen; it can verify `pen_holder = auth.uid() and
pen_expires > now()` against the row it is about to update. A client that misses heartbeats
doesn't corrupt anything — its pen simply ages out mid-save and the save 409s.

### `share-build` — the auth block

The update path grows one accepted credential with one hard condition:

```ts
const isOwner  = row.user_id && authUserId === row.user_id;
const isToken  = ownerToken && (await sha256(ownerToken)) === row.owner_token_hash;
const hasPen   = authUserId
  && row.pen_holder === authUserId && row.pen_expires > new Date().toISOString();

// Editors may write only through the pen. Owner/token keeps working exactly as today,
// for rows nobody has a session on.
if (!(isOwner || isToken || hasPen)) return 403;

if (hasPen && !isOwner && !isToken) {
  if (payload.visibility !== undefined) return 400;  // visibility is owner-only, as it already
                                                     // is for token holders (`index.ts:88`)
  if (payload.base_revision !== row.revision)        // backstop, see below
    return 409 { revision: row.revision };
  payload.author_name = row.author_name;             // the pen edits the owner's document;
}                                                    // authorship stays pinned to the owner
```

`base_revision` is a **backstop, not the conflict UX**: with the pen, an editor's autosave is
the only writer by construction, so 409 should mean "your heartbeats died and your pen lapsed" —
the client's answer is "you lost the pen; your changes are still local, take it back to push."
Old clients and the quick-share cache omit the field and keep today's behavior unchanged.

Notes that follow existing precedent rather than inventing it:

- **A grant requires an owner account.** `user_id`-less rows (anonymous, token-only) refuse
  grant/revoke/list with a "claim this build first" error; the claim flow already moves the row
  under an account, so the fix is an action the user already has.
- **`create` is never a pen act.** Taking the pen does not let anyone mint rows; a viewer who
  wants a fork saves without `existingId` and owns the new row — same as anyone today.
- **RLS read side.** `get-build` (service role, the only single-row door) must accept editors,
  and reading a `private` row via RLS needs an `exists (select 1 from build_editors ...)` clause
  added to the owner policy. The public list view must **not** gain editor or pen columns — the
  named-projection rule in `shared_builds.rs:LIST_COLUMNS` (and the `owner_token_hash` leak of
  2026-09-18 it documents) says columns enter a view by being named, and neither editor
  identities nor pen state is something the anon browse read ever needs.

### The sync meter: a third bucket, because the publish meter would starve a session

The share/vault hourly buckets meter *publishing*, and a live session is not publishing: two
people at one autosave per two seconds is one or two orders of magnitude past them inside a
30-minute call. The server adds a **sync** bucket, and — this is the part not to skip —
**only pen-gated editor updates may meter against it**; owner/token updates keep metering
share/vault, so the cheap high cap cannot be reached by anything other than a granted,
pen-holding editor. Suggested starting point: per-user-per-hour, sized so a continuous session
never trips it but a scripted client still has a ceiling. Without this bucket the feature dies
in minute six of the demo, which is the worst possible time.

### `build-editors` — one new edge function

Actions `list | grant | revoke | mine`, all under the service role, all refusing anonymous
callers:

- `grant { buildId, handle }` — owner-only (`row.user_id === auth.uid()`; token never grants
  administration, mirroring `update-build-visibility`'s stance). Resolves `handle` through
  `profiles`, refuses unknown handles, refuses self-grant, caps editors per row (suggest 5) and
  grants per build per hour to blunt enumeration of handles.
- `revoke { buildId, userId }` — owner-only; revoking also clears the pen if that user holds it.
- `list { buildId }` — owner and editors (a participant should see who else is in).
- `mine {}` — the caller's editorships with the joined row summaries, which is the "Shared with
  me" vault tab. The vault's own read stays `user_id = eq.<uid>`; a shared-with-me read through
  that PostgREST filter would need an editor-aware policy, and one small function read beats
  widening RLS on the table everyone reads.

## The live session: presence, autosave, live apply

**Channel.** Realtime, one channel per row (`build:<id>`), joined by the owner and granted
editors. Presence state carries `{ user, holding_pen }`; an `ask` broadcast surfaces "Sam asked
for the pen" on the holder's banner. Nothing about authority travels on this channel — see why
above.

**Out (the pen-holder's autosave).** Debounced ~1–2 s after typing settles, skipped when the
fingerprint is unchanged (the `owner_store::QuickShare` fingerprint machinery is the existing
proof that "did my build actually change" is cheap to answer), and **omits the preview image** —
`share-build` keeps the existing card when the field is absent, so cards refresh on deliberate
saves only. A refused sync (lapsed pen, meter spent, offline) lands as a visible
"Not saved — retrying" state and blocks nothing locally: local persistence is untouched by all of
this, and a failed session save is a banner, never a silent drop.

**In (the viewers' live apply).** The out-side piggybacks `revision` on the row; the broadcast is
just `{ revision }`; viewers refetch via `get-build` and apply. Broadcasting the whole envelope
would save a round trip and is tempting; don't — `get-build` is the sanctioned single-row door
for readers (that is exactly why it exists), at this edit cadence the refetch is nothing, and the
viewer's apply path stays byte-identical to the open-a-link path, which is code that already
works and already has tests.

**Undo.** Each person's undo stack is local, so taking the pen starts a fresh undo history from
handoff, and the UI should say so when it hands over. "Undo my change from before the handover"
is a question this model does not answer — revision history on the row, if it ever comes, is
where cross-handoff rollback lives.

### What the session looks like end to end

1. A shares the build (any visibility) and grants `@sam` from the vault's People section.
2. Both open `{origin}/builds/<id>`. A takes the pen: banner — *"You have the pen · Sam is
   watching."* Sam's: *"Ada is editing · you're watching live."* Sam's planner tracks the row;
   totals move as A moves powers.
3. "Here, try it." A taps *Hand over the pen*; Sam's banner flips, A's flips to watching. Screen
   shares on the call switch to whichever screen holds the pen — both screens show the same
   build because the row is the single source.
4. Sam tabs away; the heartbeat stops; thirty seconds later the banner says *"Pen is free."*
5. The call ends. The link still points at the final build — it always did.

## Client design (this repo)

1. **`SharedBuildSummary` gains `can_edit: Option<bool>`** (the server computes it for the
   calling session in `get-build` and `build-editors` responses) and `pen: Option<{ holder,
   expires_at, you_hold: bool }>` (in `get-build` for granted callers). Absent on list-view
   reads, and the summary already ignores unknown fields (`shared_builds.rs`, the `flatten`
   note), so this is wire-compatible with every row shape today. `is_owned()` grows one input:
   `build.can_edit == Some(true) || token || (user_id && row.user_id == user_id)` — the hint
   decides which *banner* you see; every save re-derives authority server-side.
2. **Session UI on the build view** when `can_edit`: Join/Leave (joins the channel), banner with
   participants, pen state, *Take pen / Release / Ask for pen*. Viewer mode disables editing
   surfaces with a "take the pen to edit" affordance — the block is the design, not a courtesy.
3. **Grant UI** on the vault row (owner rows only): a People section — handle input, current
   editor list, revoke. Token-owned rows show the claim prompt instead.
4. **"Shared with me" tab** in the vault, fed by `mine {}`.
5. **Pen-holder autosave** as above; **editor sessions save through the pen path, not quick
   share.** Quick share's cache (`coh-planner-quick-share`) is keyed to *the build in the
   planner* and updates *the row that cache names*; for editor B that is never A's row, and
   letting a cached row belonging to someone else into it would be the cross-user hazard the
   sign-out clear already guards against. The row's id lands in `library_id`
   (`build_session.rs:link_library`) on open, which is what the pen path saves against. Quick
   share keeps creating B's own rows, exactly as today.
6. **Lost-pen handling**: a 409 mid-session means the heartbeat died — "You lost the pen; your
   changes are still here. [Take the pen]" (which succeeds only if the pen is now free). Never a
   silent retry.
7. **Desktop is out of scope for sessions.** Desktop reads shared rows fine (the banner says
   "open in the browser to join live"); the pen and autosave paths are web-only in v1. Nobody is
   doing a collab video on desktop.

## What is deliberately not here

**Simultaneous editing (two pens)** is out, and the line is worth stating honestly: the pen
model means *live collaboration, one driver at a time*, not *edit at once*. When the product ever
wants true concurrency, the cheap path is not Google-Docs operational transform — that machinery
exists because text is a position-shifted sequence. A build is keyed assignments (power in slot
X, enhancement rank, PP pool), so a per-slot last-writer-wins CRDT over the same Realtime channel
would converge with no transformation math at all; ordered-list moves and merged deletions stay
"pick one" cases. That is a different product surface than this one and nothing here has to
pre-build for it.

**Change requests ("Sam proposed 4 changes; accept or reject")** are phase 2, and they are the
asynchronous answer for the *non-granted* — the forum buddy who read the build and has ideas but
was never handed the pen. A `build_proposals` row (base revision + envelope + status) is a create
any signed-in reader can file and only the owner can act on, rendered as a semantic per-item diff
(the skif envelope carries stable ids, so the review screen can say "moved Hasten to slot 6"
instead of showing JSON). Note what the pen already covers: async collaboration *by a granted
editor* is just "take the pen when nobody's holding it" — no session needed, the mutex is the
whole protocol. Proposals are for contributions that arrive from outside the grant list.

## Staging

1. **The live session, whole**: grants (`build_editors` + `build-editors` + People UI +
   "Shared with me"), pen (columns + `hold-pen`/`release-pen` + share-build auth branch), live
   apply (channel + banners + viewer-mode edit block), autosave, and the sync meter — **with the
   meter, as one phase.** Each is load-bearing for the demo; the meter is load-bearing for the
   demo not dying.
2. **Proposals** for non-granted contributors: `build_proposals`, the semantic diff review
   surface, accept/reject.
3. **Not planned**: two pens (see "What is deliberately not here").

## Open questions before any of it is built

- **Attribution.** Authorship pins to the owner; does the row gain `last_edited_by` so a card or
  detail view can credit the pen? Sessions make contributors visible in the moment; whether they
  persist into history is a display decision.
- **May editors invite editors?** Proposal says owner-only. Revisit only if real co-authors ask.
- **Notification.** A grant today is invisible to the grantee until they open the vault tab. A
  link pasted into the call chat covers v1; anything richer needs a notification surface that
  doesn't exist yet.
- **Sync meter caps.** Pick with the server owner; they depend on what a session actually costs
  the backend per update, which the current buckets have never had to answer.
- **Private + editors interaction.** A private row shared with an editor must stay invisible in
  search (it is, via the `visibility = 'public'` view filter) and readable only through
  `get-build` — worth stating in the function's tests either way.
- **Pen expiry vs. tab-sleep.** Browsers throttle timers in background tabs; a pen-holder who
  switches tabs mid-call may lose the pen to expiry while still meaning to drive. Either the
  heartbeat survives via the Realtime socket's own liveness (it should — the socket outlives
  throttled timers) or the release is too eager. Measure before shipping; this is the one
  mechanism where the implementation detail is the user experience.
