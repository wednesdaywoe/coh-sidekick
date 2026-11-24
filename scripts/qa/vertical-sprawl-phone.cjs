#!/usr/bin/env node
/**
 * QA check for the planner's vertical layout, phone (vertical-sprawl items 3, 4, 6, 7 on
 * touch; accolades-and-pool-headers items 1, 2 on touch).
 *
 * Needs serve.cjs running on 8787 (see the qa-check skill). Run:
 *   QA_DEVICE='Pixel 7' node scripts/qa/vertical-sprawl-phone.cjs
 *
 * What it walks:
 *  1. the Stats stack carries the Incarnates panel in the authored order (Powers,
 *     Incarnates, Available) with the socket row and no accolade strip inside it;
 *  2. the Pools / Epic triggers stay one row tall at phone width, and the set headers hold
 *     one line with the full name in the title;
 *  3. Build Settings in the More sheet carries the Accolades row and its picker opens;
 *  4. the power details sheet opens on the damage-first defaults and a tap on a section
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

  // ---- 1. the Stats stack carries the panel, sockets only ----------------------------
  await page.locator('nav.mobile-nav button', { hasText: 'Stats' }).tap();
  const stack = page.locator('.mobile-stack');
  const panelFor = (title) => stack.locator('.panel').filter({ has: page.locator('.panel-title', { hasText: title }) });
  const incarnates = panelFor('Incarnates').first();
  await incarnates.waitFor({ state: 'visible' });
  const titles = await stack.locator('.panel-title').evaluateAll((els) => els.map((e) => e.textContent.trim()));
  const at = titles.findIndex((t) => t.includes('Incarnates'));
  assert.ok(at > 0, `the panel is in the phone stack, got ${JSON.stringify(titles)}`);
  assert.ok(!titles[at].includes('Accolades'), `the panel is titled Incarnates, got "${titles[at]}"`);
  assert.match(titles[at - 1], /Powers/, 'directly after Powers, the authored order');
  assert.match(titles[at + 1], /Available/, 'and Available follows it');
  // A folded panel shows only its title; open it to read the body.
  const incBodyHas = async (re) =>
    re.test(await incarnates.evaluate((e) => e.textContent));
  if ((await incarnates.locator('.incarnate-socket').count()) === 0) {
    await incarnates.locator('.panel-fold').tap();
    await page.waitForTimeout(200);
  }
  assert.ok(await incarnates.locator('.incarnate-socket').count() > 0,
    'the socket row rides the panel');
  assert.ok(!(await incBodyHas(/accolades/i)), 'the accolade strip has left the panel');
  await incarnates.scrollIntoViewIfNeeded();
  await qa.shot('01-phone-incarnates-panel');

  // ---- 2. the set adders are one row at phone width; headers hold one line -----------
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
  const nameInfo = await pools.locator('.powerset-group__name').evaluateAll((els) =>
    els.map((e) => ({
      text: e.textContent.trim(),
      title: e.getAttribute('title'),
      whiteSpace: getComputedStyle(e).whiteSpace,
      oneLine: e.scrollHeight <= e.clientHeight + 1 && e.clientHeight <= 24,
    })));
  assert.ok(nameInfo.length > 0, 'the pools panel shows set headers');
  for (const n of nameInfo) {
    assert.equal(n.whiteSpace, 'nowrap', `"${n.text}" must not wrap`);
    assert.ok(n.oneLine, `"${n.text}" must hold one line, got ${JSON.stringify(n)}`);
    assert.equal(n.title, n.text, `"${n.text}" title carries the full name`);
  }
  await qa.shot('02-phone-set-adders');

  // ---- 3. Build Settings in the More sheet carries the Accolades row ------------------
  // Popovers render inline in the sheet (PopoverInline): no trigger to tap, the form is
  // just there, and the Accolades row rides its foot.
  await page.locator('nav.mobile-nav button', { hasText: 'More' }).tap();
  const sheet = page.locator('.mobile-sheet');
  await sheet.waitFor({ state: 'visible' });
  const bs = sheet.locator('.popover-inline[aria-label="Build Settings"]');
  await bs.scrollIntoViewIfNeeded();
  const strip = bs.locator('.picker-strip');
  assert.equal(await strip.count(), 1, 'an Accolades row rides the foot of Build Settings');
  assert.match(await strip.innerText(), /accolades/i);
  const summary = (await strip.locator('.picker-strip__power').innerText()).trim();
  assert.match(summary, /^(none|\d+ of \d+)$/, `row carries the enabled summary, got "${summary}"`);
  await qa.shot('03-phone-build-settings-accolades');
  await strip.locator('.picker-strip__open').tap();
  const accPicker = page.locator('.floating-panel[aria-label="Accolades"]');
  await accPicker.waitFor({ state: 'visible' });
  assert.ok(await accPicker.locator('.accolade-row').count() > 0, 'the picker lists the toggles');
  await qa.shot('04-phone-accolades-picker');
  await accPicker.locator('.modal-close').tap();
  await accPicker.waitFor({ state: 'hidden' });

  // ---- 4. the details sheet folds, by tap, on the fresh defaults ---------------------
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
    })));
  assert.ok(f.length >= 2, `the sheet's card has foldable sections, got ${JSON.stringify(f)}`);
  const OPEN = new Set(['Effects', 'Cost and timing', 'Damage']);
  for (const s of f) {
    assert.equal(s.expanded, OPEN.has(s.title),
      `${s.title} should be ${OPEN.has(s.title) ? 'open' : 'folded'} by default, got ${s.expanded}`);
  }
  await qa.shot('05-phone-sheet-defaults');

  // The two card-level sections are in the same sweep — both must show folded. Say it by
  // name so a regression points at the section, not at a bare loop count.
  for (const title of ['Description', 'Adjusters']) {
    const s = f.find((x) => x.title === title);
    if (s) assert.equal(s.expanded, false, `${title} starts folded on a fresh profile`);
  }
  assert.ok(f.some((s) => s.title === 'Description'), 'the prose carries a foldable heading');

  // Tap the heading — the coarse-pointer seat gets the fold.
  const effects = card.locator('h4.is-foldable', { hasText: 'Effects' }).first();
  const seat = await effects.boundingBox();
  assert.ok(seat.height >= 36, `the heading is a finger-sized target, got ${seat.height}px`);
  await effects.tap();
  await page.waitForTimeout(200);
  const after = await effects.getAttribute('aria-expanded');
  assert.equal(after, 'false', 'a tap folds the section');
  let stored = await page.evaluate(() => localStorage.getItem('sk-info-folds'));
  assert.match(stored, /"info:effects":true/, `tap persisted the fold, got ${stored}`);
  await qa.shot('06-phone-effects-folded');

  // Opening Description by tap writes its card: key and the prose returns.
  const desc = card.locator('h4.is-foldable', { hasText: 'Description' }).first();
  await desc.tap();
  await page.waitForTimeout(200);
  assert.equal(await desc.getAttribute('aria-expanded'), 'true', 'a tap opens the description');
  assert.ok(await card.locator('.info-description p, .short-help').first().isVisible(),
    'the prose returns with the fold');
  stored = await page.evaluate(() => localStorage.getItem('sk-info-folds'));
  assert.match(stored, /"card:description":false/, `the open description persisted, got ${stored}`);

  // Adjusters, on a power that has inputs: tap opens the switches back in. The sheet's
  // close button is in the Modal's header, OUTSIDE the .power-info-sheet content div.
  const sheetClose = page.locator('.modal-header .modal-close');
  await sheetClose.tap();
  await page.waitForTimeout(250);
  await page.getByRole('button', { name: 'About Cloak of Darkness' }).first().tap();
  const adjCard = page.locator('.power-info-sheet');
  await adjCard.waitFor({ state: 'visible' });
  await page.waitForTimeout(350);
  const adj = adjCard.locator('h4.is-foldable', { hasText: 'Adjusters' }).first();
  assert.equal(await adj.getAttribute('aria-expanded'), 'false', 'the adjusters start folded');
  await adj.tap();
  await page.waitForTimeout(200);
  assert.ok(await adjCard.locator('.adjuster-row, .adjuster-label').first().isVisible(),
    'the adjuster inputs return with the fold');
  stored = await page.evaluate(() => localStorage.getItem('sk-info-folds'));
  assert.match(stored, /"card:adjusters":false/, `the open adjusters persisted, got ${stored}`);
  await qa.shot('07-phone-adjusters-open');
  await sheetClose.tap();

  console.log('phone sprawl check passed');
});
