# URL-fragment corpus

The `#…` on a share link, as a decoder has to find it. Emitted by
`scripts/emit-import-link-fixtures.ts`, which read the beta's encoder out of `src/` and was
deleted on 2026-09-25 — so this corpus is frozen and cannot be re-emitted here.

**Provenance is the whole point.** These fragments are written by the beta's shipped encoder
(`src/utils/import-url.ts`) over two corpora that were themselves authored elsewhere — the eight
`.skif` files the beta planner exported (`../skif/v4`) and the seven builds the Homecoming game
client wrote (`../buildsave`). Nothing in `crates/` encodes a fragment, so this is a format
oracle rather than a round trip: a codec graded against its own output states only that it is
self-consistent.

## The arms

| | what it is | who writes one |
|---|---|---|
| `planner/` (8) | raw deflate + base64 over a v4 `.skif` | the beta's `encodeBuildToHash` — every share link and `/import#…` link |
| `game-export/` (7) | plain base64 over `/buildsave` text, no compression | **nobody yet** — the Vault, prospectively |
| `game-export-deflated/` (1) | the same text through the beta encoder | nobody yet; it is what a link would be if one carried an export |
| `zlib/` (1) | a v4 `.skif` under a zlib header instead of raw deflate | nobody — the beta's decoder accepts it, so this one grades that arm |
| `vault/` (1) | raw deflate + URL-safe base64 over the game client's JSON build | **Homecoming's Vault, live** — taken from a real `/build-export/…/sidekick` page on 2026-11-03 |

**Three of the five arms have no producer, and that is stated rather than hidden.** The formats
actually in circulation are `planner/` and `vault/`. The Vault went live sending the client's JSON
build, not the `/buildsave` text it was expected to, so the game-export arms still have no writer,
and the beta cannot read either of them. The Vault's page forwards to `/#…` and its build names no
server, which is why `open_route` sends it to Homecoming. The uncompressed game-export arm is named as a
requirement by REBUILD-PROGRESS and is *rejected* by the beta's
own decoder, whose uncompressed branch takes a payload only if it starts with `{`. The Rust
decoder accepts all five because it decides transport and payload separately instead of pairing
them, which is what makes the game-export arms fall out rather than needing a case each.

## What the corpus cannot see

No fragment here was written by a game client. `/buildsave` writes a file and prints
`BuildSaved`; no Homecoming build in this tree emits a URL, and none of the chat logs carries
one. So the *encoding* of a game-export link is an assumption, and the day a client ships one
this corpus is what has to be re-cut against it.
