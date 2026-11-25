# Letting two or more people edit one shared build

Status: **proposal, nothing built.** Written 2026-11-25 in response to a user request for
"Google Docs for builds — several people working on one character at the same link."

This doc states what already exists (with file and line references), the design decision it
recommends, what it rejects and why, and the staging. The enforcement half lands in the Supabase
edge functions, which do not live in this repo; the client half lands here.

## The ask, and the accurate version of the premise

The user asked whether someone can grant another account permission to edit a build. The premise
behind the ask — "the planner already live-edits, because the link keeps pointing at the build as
it changes" — is true in a specific sense: **Copy short link** (`quick_share`,
`crates/app/src/cloud/save_build.rs:405`) creates one `unlisted` row and then **updates that same
row in place** on every re-copy, which is what the fingerprint cache in
`crates/app/src/cloud/owner_store.rs` exists to make free. The URL never changes and always
resolves to the latest save (`{origin}/builds/{id}`, `browser.rs:copy_url`).

What exists is a shared, stable, re-writeable document with exactly one writer. What does not
exist is a second writer, a grant mechanism, or any merge — an update POSTs the whole skif v5
`build_json` envelope, so concurrent editing today is last-write-wins with silent loss.

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
author-profile feature reads it), and the vault reads `user_id = eq.<uid>`
(`shared_builds.rs:my_builds_wire`, line 469).

## Decision: a per-row editor ACL, not a shared secret

**Recommended: Option B — an `build_editors` ACL table checked server-side at update time.**
Granting by account (handle → `user_id`) rather than by secret is what makes the feature
revocable, attributable, and safe to hand to someone you don't fully trust.

### Rejected: sharing the owner token (Option A)

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

## Server design (Supabase repo; not this one)

### The table

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

### `share-build` — the auth block

The update path grows a third accepted credential, and the editor branch is the *narrow* one:

```ts
const isOwner  = row.user_id && authUserId === row.user_id;
const isToken  = ownerToken && (await sha256(ownerToken)) === row.owner_token_hash;
const isEditor = !isOwner && !isToken && authUserId
  && await isEditorOf(supabase, row.id, authUserId);   // one indexed PK lookup

if (!isOwner && !isToken && !isEditor) return 403;

if (isEditor) {
  // Editors update content and nothing else.
  if (payload.visibility !== undefined) return 400;    // visibility is owner-only, and the
                                                       // token branch already refuses it too
  payload.author_name = row.author_name;               // pin authorship to the owner; an
  payload.author_id   = row.user_id;                   // editor save must not re-author the build
}
// (exact author-column names per the current share-build overwrite-everything update.)
```

Notes that follow existing precedent rather than inventing it:

- **A grant requires an owner account.** `user_id`-less rows (anonymous, token-only) refuse
  grant/revoke/list with a "claim this build first" error; the claim flow already moves the row
  under an account, so the fix is an action the user already has.
- **`create` is never an editor act.** An editor who wants a fork saves without `existingId` and
  owns the new row — same as anyone today.
- **Rate limits stay per-user.** An editor updating the shared row burns the *editor's* slot,
  which is right: the meter exists to cap one person's demand on the backend, not one row's.
- **RLS read side.** `get-build` (service role, the only single-row door) must accept editors,
  and reading a `private` row via RLS needs an `exists (select 1 from build_editors ...)` clause
  added to the owner policy. The public list view must **not** gain editor columns — the
  named-projection rule in `shared_builds.rs:LIST_COLUMNS` (and the `owner_token_hash` leak of
  2026-09-18 it documents) says columns enter a view by being named, and editor identities are
  not something the anon browse read ever needs.

### `build-editors` — one new edge function

Actions `list | grant | revoke | mine`, all under the service role, all refusing anonymous
callers:

- `grant { buildId, handle }` — owner-only (`row.user_id === auth.uid()`; token never grants
  administration, mirroring `update-build-visibility`'s stance). Resolves `handle` through
  `profiles`, refuses unknown handles, refuses self-grant, caps editors per row (suggest 5) and
  grants per build per hour to blunt enumeration of handles.
- `revoke { buildId, userId }` — owner-only.
- `list { buildId }` — owner and editors (an editor should be able to see who else is in).
- `mine {}` — the caller's editorships with the joined row summaries, which is the "Shared with
  me" vault tab. The vault's own read stays `user_id = eq.<uid>`; a shared-with-me read through
  that PostgREST filter would need an editor-aware policy, and one small function read beats
  widening RLS on the table everyone reads.

### Stage 2 conflict guard (ship with the ACL, not after it)

Two writers on a whole-document blob is a data-loss machine the moment two people exist. The row
gains `revision bigint not null default 0`; every accepted update increments it; an update may
carry `base_revision`, and a mismatch answers **409** with the current row. Omitting
`base_revision` keeps last-write-wins for callers that have not read the row recently — i.e. old
clients and the quick-share cache keep working unchanged — but the new client always sends it for
editor saves, where the silent clobber is most likely and most confusing ("I saved, their save ate
mine, nobody was told"). 409 is also the honest answer to the user's literal ask: "at the same
time" means the app must be able to say *you both changed this since you opened it*.

## Client design (this repo)

Small, because the server is the decider:

1. **`SharedBuildSummary` gains `can_edit: Option<bool>`**, present only where the server computes
   it for the calling session (`get-build`, `build-editors` responses). Absent on list-view reads,
   and the summary already ignores unknown fields (`shared_builds.rs`, the `flatten` note), so
   this is wire-compatible with every row shape today. `is_owned()` grows one input:
   `build.can_edit == Some(true) || token || (user_id && row.user_id == user_id)`.
   The hint is exactly that — a hint. Every save re-derives authority server-side.
2. **Grant UI** on the vault row (owner rows only): a People section — handle input, current
   editor list, revoke. Token-owned rows show the claim prompt instead.
3. **"Shared with me" tab** in the vault, fed by `mine {}`.
4. **Editor save path is the save dialog, not quick share.** Quick share's cache
   (`coh-planner-quick-share`) is keyed to *the build in the planner* and updates *the row that
   cache names*; for editor B that is never A's row, and letting a cached row belonging to someone
   else into it would be the cross-user hazard the sign-out clear already guards against. B opens
   A's link (the row's id lands in `library_id`, `build_session.rs:link_library`), edits, and
   "Update saved build" sends the update the server now authorizes. Quick share keeps
   creating B's own rows, exactly as today.
5. **409 handling** on editor saves: "This build changed since you opened it — [load theirs] [save
   mine as new copy]." Never a silent retry.

## Staging

1. **ACL + narrow editor branch + can_edit + grant UI + "Shared with me" + revision guard** — the
   whole literal ask, plus the guard rail it needs to not eat anyone's build.
2. *(Maybe)* presence: "Ada is editing this build" via Supabase Realtime on a per-row channel.
   Cheap-ish once the ACL exists, and much of the felt "Google Docs" quality.
3. **Not planned: true simultaneous co-editing** (live cursors, per-field merge, CRDT over the
   skif envelope). The build travels as one opaque document; merging inside it is a different
   product surface than this one. Say "collaborate on a build" not "edit at once" in any reply to
   the user.

## Open questions before any of it is built

- **Authorship display.** Pinning `author_name` to the owner is proposed above; the alternative
  is a `last_edited_by` column the card and detail view can show. Pin-without-attribution is
  simple but hides contributors; pick one before building the detail surface.
- **May editors invite editors?** Proposal says owner-only. Revisit only if real co-authors ask.
- **Notification.** An invite today is invisible to the grantee until they open the vault tab. A
  build link pasted by hand covers v1; anything richer needs a notification surface that doesn't
  exist yet.
- **Private + editors interaction.** A private row shared with an editor must stay invisible in
  search (it is, via the `visibility = 'public'` view filter) and readable only through
  `get-build` — worth stating in the function's tests either way.
