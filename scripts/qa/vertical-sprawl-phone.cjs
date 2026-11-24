#!/usr/bin/env node
/**
 * QA check for the vertical-sprawl plan, phone layout (plan items 3, 4, 6, 7 on touch).
 *
 * Needs serve.cjs running on 8787 (see the qa-check skill). Run:
 *   QA_DEVICE='Pixel 7' node scripts/qa/vertical-sprawl-phone.cjs
 *
 * What it walks:
 *  1. the Stats stack carries the panel in the authored order (Powers, Incarnates, Available)
 *     with both strips inside it;
 *  2. the Pools / Epic triggers stay one row tall at phone width;
 *  3. the power details sheet opens on the damage-first defaults and a tap on a section
 *     heading folds it — and writes the same preference storage the desktop uses.
 *
 * Every interaction is a `.tap()`, never `.click()`, and every shot is the viewport, never
 * an element: Playwright's click dispatches MOUSE events and an element screenshot scrolls
 * via a synthesized wheel — and either one flips the emulated `(pointer: coarse)` media the
 * whole touch layer (44px seats included) is written behind. A real phone sends neither;
 * the check must not either.
 */

const assert = require('node:assert/strict');
const { run } = require('../../.claude/skills/qa-check/scripts/session.cjs');

const BUILD = 'fixtures/buildsave/homecoming/charnel.txt';

run({ build: BUILD }, async (qa) => {
  const { page } = qa;

  // ---- 1. the Stats stack carries the panel, with both strips ------------------------
  await page.locator('nav.mobile-nav button', { hasText: 'Stats' }).tap();
  const stack = page.locator('.mobile-stack');
  const panelFor = (title) => stack.locator('.panel').filter({ has: page.locator('.panel-title', { hasText: title }) });
  const incarnates = panelFor('Incarnates & Accolades').first();
  await incarnates.waitFor({ state: 'visible' });
  const titles = await stack.locator('.panel-title').evaluateAll((els) => els.map((e) => e.textContent.trim()));
  const at = titles.findIndex((t) => t.includes('Incarnates'));
  assert.ok(at > 0, `the panel is in the phone stack, got ${JSON.stringify(titles)}`);
  assert.match(titles[at - 1], /Powers/, 'directly after Powers, the authored order');
  assert.match(titles[at + 1], /Available/, 'and Available follows it');
  // A folded panel shows only its title; open it to read the body.
  const foldBtn = incarnates.locator('.panel-fold');
  const incHasAccolades = async () =>
    (await incarnates.evaluate((e) => e.textContent)).toLowerCase().includes('accolades');
  if (!(await incHasAccolades())) {
    await foldBtn.tap();
    await page.waitForTimeout(200);
  }
  assert.ok(await incHasAccolades(), 'the accolade strip rides in the panel');
  await incarnates.scrollIntoViewIfNeeded();
  await qa.shot('01-phone-incarnates-panel');

  // ---- 2. the set adders are one row at phone width ---------------------------------
  const pools = panelFor('Power Pools').first();
  await pools.scrollIntoViewIfNeeded();
  if (!(await pools.locator('.set-adder-button').count())) {
    await pools.locator('.panel-fold').tap();
    await page.waitForTimeout(200);
  }
  const adders = pools.locator('.set-adder-button');
  assert.equal(await adders.count(), 2, 'Pools and Epic triggers on the phone panel');
  const boxes = [];
  for (const a of await adders.all()) boxes.push(await a.boundingBox());
  assert.equal(new Set(boxes.map((b) => Math.round(b.y))).size, 1, 'one row at phone width');
  assert.ok(boxes.every((b) => b.height <= 48), `row height ${boxes.map((b) => b.height)}`);
  await qa.shot('02-phone-set-adders');

  // ---- 3. the details sheet folds, by tap, on the fresh defaults ---------------------
  assert.equal(await page.evaluate(() => localStorage.getItem('sk-info-folds')), null,
    'nothing stored before the first toggle');
  await page.locator('nav.mobile-nav button', { hasText: 'Build' }).tap();
  await page.waitForTimeout(200);
  await page.getByRole('button', { name: 'About Dark Embrace' }).first().tap();
  const card = page.locator('.power-info-sheet');
  await card.waitFor({ state: 'visible' });
  await page.waitForTimeout(350); // the sheet opens on a scale animation; a box measured
  //                                   mid-animation reads as a fraction of its real seat
  const f = await card.locator('h4.is-foldable').evaluateAll((els) =>
    els.map((e) => ({
      title: e.textContent.replace('▼', '').trim(),
      expanded: e.getAttribute('aria-expanded') === 'true',
    })),
  );
  assert.ok(f.length >= 2, `the sheet's card has foldable sections, got ${JSON.stringify(f)}`);
  const OPEN = new Set(['Effects', 'Cost and timing', 'Damage']);
  for (const s of f) {
    assert.equal(s.expanded, OPEN.has(s.title),
      `${s.title} should be ${OPEN.has(s.title) ? 'open' : 'folded'} by default, got ${s.expanded}`);
  }
  await qa.shot('03-phone-sheet-defaults');

  // Tap the heading — the coarse-pointer seat gets the fold.
  const effects = card.locator('h4.is-foldable', { hasText: 'Effects' }).first();
  const seat = await effects.boundingBox();
  assert.ok(seat.height >= 36, `the heading is a finger-sized target, got ${seat.height}px`);
  await effects.tap();
  await page.waitForTimeout(200);
  const after = await effects.getAttribute('aria-expanded');
  assert.equal(after, 'false', 'a tap folds the section');
  const stored = await page.evaluate(() => localStorage.getItem('sk-info-folds'));
  assert.match(stored, /"info:effects":true/, `tap persisted the fold, got ${stored}`);
  await qa.shot('04-phone-effects-folded');

  console.log('phone sprawl check passed');
});
