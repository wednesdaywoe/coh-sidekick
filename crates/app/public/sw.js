// Retires the beta's service worker. Nothing in this app registers it.
//
// The beta (vite-plugin-pwa, `registerType: 'prompt'`) registered /sw.js at scope / and served
// every navigation from its precache. A returning beta visitor's browser keeps doing that after
// coh-sidekick.com moves to this app, until the worker at /sw.js changes. The browser re-checks
// this URL on navigation, finds this file, and installs it. `skipWaiting` lets it take over
// without the beta's update prompt, which would otherwise hold it in "waiting" indefinitely.
// Once active it drops the beta's caches, unregisters, and reloads the open tabs, which then
// load from the network: this app.
//
// This file must be served as JavaScript at exactly /sw.js. The SPA fallback answers a missing
// /sw.js with index.html, the browser rejects that as a script, and the beta stays in charge.

self.addEventListener('install', () => {
  self.skipWaiting();
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      const keys = await caches.keys();
      await Promise.all(keys.map((key) => caches.delete(key)));
      await self.registration.unregister();
      const windows = await self.clients.matchAll({ type: 'window' });
      for (const client of windows) {
        client.navigate(client.url);
      }
    })(),
  );
});
