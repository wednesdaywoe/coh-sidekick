# Moving the app from next.coh-sidekick.com to coh-sidekick.com

Decided 2026-11-18: retire the React beta and serve this app at the apex, as part of the 1.0.0
release. When this was decided, 7.7% of visitors were still on the beta.

## What the code does (branch `apex-cutover`)

- **`wrangler.toml`** routes the Worker to both `coh-sidekick.com` and `next.coh-sidekick.com`.
- **`crates/app/public/sw.js`** retires the beta's service worker. The beta registered `/sw.js`
  and served every page from its cache, so without this file a returning beta visitor keeps
  getting the beta. The browser fetches this replacement, which clears the beta's caches,
  unregisters itself and reloads the tab.
- **The beta's working build** opens once on a beta user's first visit (`BetaCarryOverHost` in
  `build_io.rs`). The beta's own key is only read, never removed.
- **`next.coh-sidekick.com` hands off.** `index.html` on that host sends the browser's storage
  to `coh-sidekick.com/handoff.html`, then redirects to the same path on the apex. The handoff
  only fills keys the apex doesn't have. Owner tokens and favourites are merged.
- **`crates/app/index.html`** carries the beta's og and twitter tags. The beta's `build-og`
  Worker on `coh-sidekick.com/builds/*` rewrites these tags in place for shared builds, so its
  link previews keep working with no change to that Worker.
- **`crates/app/public/`** also holds the beta's root files: favicons under `/img/`,
  `manifest.json` (installed beta apps launch from it), `og-image.png`, `robots.txt` and
  `sitemap.xml`.

Already pointing at the apex, unchanged: the desktop app's `SITE_ORIGIN`, the feedback Worker's
origin list, and Supabase's Site URL.

## Release day, in order

1. **Merge to `main` without deploying yet.** Pushing to `main` runs `deploy-web.yml`, and its
   apex route fails while GitHub Pages' DNS records exist. A failed deploy changes nothing, but
   it is noise. Either merge right after step 2, or run step 3 by hand.
2. **Cloudflare → coh-sidekick.com → DNS:** delete the apex records that point at GitHub Pages
   (A `185.199.108–111.153`, any AAAA, or a CNAME to `wednesdaywoe.github.io`). The site is
   down from here until step 3 finishes, which is a few minutes.
3. **Deploy:** push `main`, or run `scripts/deploy-web.sh`. Wrangler creates the apex record and
   certificate.
4. **Check it:**
   - `curl -sI https://coh-sidekick.com/sw.js` returns `content-type: application/javascript`,
     not `text/html`.
   - `curl -s https://coh-sidekick.com/ | grep og:title` finds the tag.
   - `curl -s https://coh-sidekick.com/builds/<a public id> | grep og:title` shows that build's
     name, which means the `build-og` Worker is still running in front.
   - In a browser that has used the beta, opening the site lands in this app with the beta's
     build open.
   - In a browser that has used `next`, opening `next.coh-sidekick.com/builds/<id>` lands on
     the apex, still signed in, with the same working build.
5. **`www`:** GitHub Pages currently redirects `www` to the apex. Add a Cloudflare Redirect Rule
   that sends `www.coh-sidekick.com/*` to `https://coh-sidekick.com/${1}` (301, keep the query
   string), and leave `www`'s DNS record proxied.
6. **Beta repo (`coh-sidekick-beta`):**
   - Settings → Pages: remove the custom domain and unpublish.
   - Disable `deploy.yml`, and delete `public/CNAME`, so nothing claims the domain again.
   - Archive the repo once nobody needs it.
   - Note added 2026-11-28: the Supabase schema and edge functions still lived in the beta when
     it was archived, so they could no longer be pushed. They moved to this repo's `supabase/`
     that day (the `supabase-move` plan).
7. **Supabase → Auth → URL Configuration:** keep `https://coh-sidekick.com/**` in the redirect
   list. `next` can stay listed until it is retired.

## Later

After a few months, when `next` traffic is negligible, replace the `next` route with a Redirect
Rule to the apex, and delete `handoff.html` and the handoff script in `index.html`. Keep `sw.js`
for at least a year: any browser that hasn't visited since the beta still has its worker.

## Rolling back

Put the GitHub Pages DNS records back, remove the apex route from `wrangler.toml`, and redeploy.
The beta's storage is intact, because the carry-over only reads it. The kill switch will have
unregistered the beta's service worker in browsers that visited, and the beta registers it again
on their next visit.
