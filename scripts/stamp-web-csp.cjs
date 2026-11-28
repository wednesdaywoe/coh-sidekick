#!/usr/bin/env node
/**
 * Writes the web build's Content-Security-Policy into `_headers`, the file Cloudflare reads to
 * add headers to every response. Runs as the last step of `npm run build:web`, on the BUILT
 * pages, so the inline scripts' hashes are always of the HTML that ships. The hosts come from
 * the same SIDEKICK_* variables the wasm was compiled with: a build without Sentry or Supabase
 * makes no call to them, so its policy names neither.
 *
 *   node scripts/stamp-web-csp.cjs [<bundle dir>]
 */

const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

const DIR = path.resolve(
  process.argv[2] || path.join(__dirname, '../target/dx/Sidekick/release/web/public'),
);
// Mirrors feedback.rs's WORKER_URL.
const FEEDBACK = 'https://coh-planner-feedback.wedswoe.workers.dev';

function originOf(name) {
  const raw = process.env[name];
  if (!raw) return null;
  return new URL(raw).origin;
}

// Every inline <script> (no src=) in every page of the bundle, hashed the way CSP hashes them:
// sha256 of the element's exact text.
function inlineScriptHashes() {
  const hashes = new Set();
  for (const file of fs.readdirSync(DIR).filter((f) => f.endsWith('.html'))) {
    const html = fs.readFileSync(path.join(DIR, file), 'utf8');
    for (const m of html.matchAll(/<script(?![^>]*\bsrc=)[^>]*>([\s\S]*?)<\/script>/gi)) {
      if (!m[1]) continue;
      hashes.add(`'sha256-${crypto.createHash('sha256').update(m[1], 'utf8').digest('base64')}'`);
    }
  }
  return [...hashes];
}

const supabase = originOf('SIDEKICK_SUPABASE_URL');
const sentry = originOf('SIDEKICK_SENTRY_DSN');

const policy = [
  "default-src 'self'",
  // 'unsafe-eval' because dioxus-web runs every document::eval through `new Function`
  // (document.rs:220); the desktop policy pays the same cost for the same reason.
  `script-src 'self' 'unsafe-eval' 'wasm-unsafe-eval' ${inlineScriptHashes().join(' ')}`,
  "style-src 'self' 'unsafe-inline'",
  "font-src 'self' data:",
  // The one avatar host cloud::avatar::avatar_src allows; data:/blob: are the app's own
  // previews and downloads.
  "img-src 'self' data: blob: https://cdn.discordapp.com",
  ['connect-src', "'self'", supabase, sentry, FEEDBACK].filter(Boolean).join(' '),
  // coh-sidekick.com is the handoff frame index.html opens on next.coh-sidekick.com; the rest
  // is the in-app donation widget.
  "frame-src 'self' https://coh-sidekick.com https://buymeacoffee.com https://www.buymeacoffee.com",
  // handoff.html is framed by the old address.
  "frame-ancestors 'self' https://next.coh-sidekick.com",
  "worker-src 'self'",
  "manifest-src 'self'",
  "base-uri 'self'",
  "form-action 'self'",
  "object-src 'none'",
].join('; ');

if (!fs.existsSync(path.join(DIR, 'index.html'))) {
  console.error(`stamp-web-csp: no index.html in ${DIR}`);
  process.exit(1);
}
fs.writeFileSync(path.join(DIR, '_headers'), `/*\n  Content-Security-Policy: ${policy}\n`);
console.log(`stamp-web-csp: wrote ${path.relative(process.cwd(), path.join(DIR, '_headers'))}`);
for (const [name, origin] of [['Supabase', supabase], ['Sentry', sentry]]) {
  if (!origin) console.log(`stamp-web-csp: ${name} not configured, so not in connect-src`);
}
