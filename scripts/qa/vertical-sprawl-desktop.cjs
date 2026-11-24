#!/usr/bin/env node
/**
 * QA check for the vertical-sprawl plan, desktop layout (plan items 3, 4, 6, 7).
 *
 * Needs serve.cjs running on 8787 (see the qa-check skill). Run:
 *   node scripts/qa/vertical-sprawl-desktop.cjs
 *
 * What it walks, in order:
 *  1. the incarnate sockets + accolades live in their own panel and Available shows neither,
 *     and the panel can be hidden and put back from the Display menu;
 *  2. the Pools / Epic triggers are one row tall, also at the panel's minimum width;
 *  3. a fresh profile opens the Info card with Damage, Effects and Cost and timing and
 *     nothing else, folding one section survives a reload, follows to another power, to the
 *     Details tab and into the pool picker's preview, and the heading answers to Enter.
 */

const assert = require('node:assert/strict');
const { run, ROOT } = require('../../.claude/skills/qa-check/scripts/session.cjs');

const BUILD = 'fixtures/buildsave/homecoming/charnel.txt';
const OPEN_BY_DEFAULT = new Set(['Damage', 'Effects', 'Cost and timing']);

/** Every foldable h4 of a card as { title, expanded }. */
async function folds(card) {
  return card.locator('h4.is-foldable').evaluateAll((els) =>
    els.map((e) => ({
      title: e.textContent.replace('▼', '').trim(),
      expanded: e.getAttribute('aria-expanded') === 'true',
    })),
  );
}

run({ build: BUILD, width: 1440, height: 900 }, async (qa) => {
  const { page } = qa;
  const surface = (id) => page.locator(`.surface[data-id="${id}"]`);
  const info = surface('info');
  const available = surface('available');
  const incarnates = surface('incarnates');
  const pools = surface('pools');

  // ---- 1. the strips moved into their own panel ------------------------------------
  await incarnates.scrollIntoViewIfNeeded();
  assert(await (await incarnates.evaluate((e) => e.textContent)).includes('Incarnates & Accolades'),
    'the panel is titled Incarnates & Accolades');
  assert((await incarnates.evaluate((e) => e.textContent)).includes('Accolades'),
    'the accolade strip must ride in the new panel');
  const availText = await available.evaluate((e) => e.textContent);
  assert.ok(!availText.includes('Accolades'), 'Available must no longer draw the accolade strip');
  assert.ok(!/socket/i.test(availText), 'Available must no longer draw incarnate sockets');
  // The quickbar pill for the panel (its restore route alongside the Display menu).
  console.log('quickbar pills matching "Incarnates":',
    await page.locator('.quickbar-pill', { hasText: 'Incarnates' }).count());
  await qa.shot('01-incarnates-panel', incarnates);

  // Hide it from its header, put it back from the Display menu — the routes a panel that
  // ships on the grid has (no quickbar pill unless the user pins one).
  await page.getByRole('button', { name: 'Hide Incarnates & Accolades panel' }).click();
  assert.equal(await incarnates.isVisible(), false, 'hidden means off the grid');
  const display = page.getByRole('button', { name: /Display/ });
  await display.click();
  await page
    .locator('.quickbar-menu button[title="Put Incarnates & Accolades back on the grid"]')
    .first()
    .click();
  await incarnates.waitFor({ state: 'visible' });
  await page.keyboard.press('Escape');
  await qa.shot('02-panel-restored-from-display-menu', incarnates);

  // ---- 2. the set adders are one row tall, at minimum width too --------------------
  await pools.scrollIntoViewIfNeeded();
  const adders = pools.locator('.set-adder-button');
  assert.equal(await adders.count(), 2, 'Pools and Epic triggers');
  const labels = (await adders.locator('.set-adder-button__label').allInnerTexts())
    .map((s) => s.trim().toLowerCase());
  assert.deepEqual(labels, ['pools', 'epic'], `trigger labels, got ${JSON.stringify(labels)}`);
  const rowOf = async () => {
    const boxes = [];
    for (const b of await adders.all()) boxes.push(await b.boundingBox());
    return boxes;
  };
  let boxes = await rowOf();
  assert.equal(new Set(boxes.map((b) => Math.round(b.y))).size, 1, 'both triggers share one row');
  assert.ok(boxes.every((b) => b.height <= 44), `one row tall, got ${boxes.map((b) => b.height)}`);
  await pools.scrollIntoViewIfNeeded();
  await page.mouse.wheel(0, -220); // the stat band is pinned and stays at the top while the
  //                                 page scrolls; scroll up until the pools head clears it
  await page.waitForTimeout(150);
  await qa.shot('03-set-adders-default');

  // Drag the panel narrower with its bottom-edge handle. Wheel back down first: the
  // scroll-up that freed the header for shot 03 pushed the panel's bottom — and its
  // handle — past the window edge, and a mouse.down off-window starts nothing. The drag
  // stays inside the viewport: the drag-overlay listens for pointerup on itself, so a
  // release outside the window would strand the overlay over the whole app (pre-existing,
  // reported, out of plan).
  await page.mouse.wheel(0, 240);
  await page.waitForTimeout(150);
  const widthBefore = (await pools.boundingBox()).width;
  const handle = pools.locator('.resize-handle').first();
  const hb = await handle.boundingBox();
  assert.ok(hb && hb.y + hb.height < 898, `the handle is inside the window to grab, got ${JSON.stringify(hb)}`);
  const targetX = hb.x + hb.width / 2 - 150;
  assert.ok(targetX > 10, 'the drag stays inside the window');
  await page.mouse.move(hb.x + hb.width / 2, hb.y + hb.height / 2);
  await page.mouse.down();
  await page.mouse.move(targetX, hb.y + hb.height / 2, { steps: 10 });
  await page.mouse.up();
  const widthAfter = (await pools.boundingBox()).width;
  assert.ok(widthAfter < widthBefore - 60, `the drag narrowed the panel: ${widthBefore} -> ${widthAfter}`);
  boxes = await rowOf();
  assert.equal(new Set(boxes.map((b) => Math.round(b.y))).size, 1, 'still one row at minimum width');
  assert.ok(boxes.every((b) => b.height <= 44), 'still one row tall');
  const fits = await adders.locator('.set-adder-button__label')
    .evaluateAll((els) => els.map((e) => ({ text: e.textContent, scroll: e.scrollWidth, client: e.clientWidth })));
  for (const f of fits) assert.ok(f.scroll <= f.client + 1, `"${f.text}" truncated at minimum width`);
  await pools.scrollIntoViewIfNeeded();
  await page.mouse.wheel(0, -220);
  await page.waitForTimeout(150);
  await qa.shot('04-set-adders-minimum-width');

  // ---- 3. the Info card folds, and remembers ----------------------------------------
  assert.equal(await page.evaluate(() => localStorage.getItem('sk-info-folds')), null,
    'nothing stored before the first toggle — the opens come from the code default');
  // Hover, never click: an Available row is a pick/unpick toggle and a check must not
  // rewrite the build. Hover drives the Info selection the way the panel documents.
  await available.locator('.power-row', { hasText: 'Soul Drain' }).first().hover();
  const card = info.locator('.info-power').first();
  await card.waitFor();
  let f = await folds(card);
  assert.ok(f.length >= 4, `the card has foldable sections, got ${JSON.stringify(f)}`);
  assert.ok(f.some((s) => s.title === 'Effects'), 'Soul Drain shows an Effects section to test with');
  for (const s of f) {
    assert.equal(s.expanded, OPEN_BY_DEFAULT.has(s.title),
      `${s.title} should be ${OPEN_BY_DEFAULT.has(s.title) ? 'open' : 'folded'} on a fresh profile, got ${s.expanded}`);
  }
  await qa.shot('05-info-default-folds');

  // The Damage section folds its body but keeps its head row.
  await card.locator('h4.is-foldable', { hasText: 'Damage' }).click();
  assert.equal(await card.locator('.damage-hero').isVisible(), false, 'the hero folds');
  assert.ok(await card.locator('.damage-chip').first().isVisible(), 'the chips stay with the heading');
  await card.locator('h4.is-foldable', { hasText: 'Damage' }).press('Enter');
  assert.ok(await card.locator('.damage-hero').isVisible(), 'Enter expands the folded section');

  // Fold Effects; the preference writes itself to storage.
  await card.locator('h4.is-foldable', { hasText: 'Effects' }).click();
  f = await folds(card);
  assert.equal(f.find((s) => s.title === 'Effects').expanded, false);
  const stored = await page.evaluate(() => localStorage.getItem('sk-info-folds'));
  assert.match(stored, /"info:effects":true/, `fold persisted, got ${stored}`);
  await qa.shot('06-effects-folded');

  // Details tab: everything starts folded; opening Accepts is a preference too.
  await info.getByRole('tab', { name: 'Details', exact: true }).click();
  f = await folds(card);
  for (const s of f) assert.equal(s.expanded, false, `Details starts folded, ${s.title} was open`);
  const accepts = card.locator('h4.is-foldable', { hasText: 'Accepts' });
  if (await accepts.count()) await accepts.click();
  else await card.locator('h4.is-foldable').first().click();
  await info.getByRole('tab', { name: 'Info', exact: true }).click();

  // Reload: the build is re-opened, the folds come from storage.
  await page.reload();
  await page.locator('.desktop-grid .surface:visible').first().waitFor({ timeout: 60_000 });
  if (!(await available.locator('.power-row').count())) await qa.openBuild(BUILD);
  await available.locator('.power-row', { hasText: 'Siphon Life' }).first().hover(); // a DIFFERENT power
  await card.waitFor();
  f = await folds(card);
  assert.equal(f.find((s) => s.title === 'Effects').expanded, false, 'the fold follows another power');
  assert.equal(f.find((s) => s.title === 'Damage').expanded, true, 'untouched sections keep the default');
  await info.getByRole('tab', { name: 'Details', exact: true }).click();
  f = await folds(card);
  if (await accepts.count()) {
    assert.equal(f.find((s) => s.title === 'Accepts').expanded, true, 'the Details fold survives too');
  }
  await info.getByRole('tab', { name: 'Info', exact: true }).click();

  // The pool picker's preview honours the same preference.
  await adders.first().click(); // the Pools trigger
  const picker = page.locator('.pool-picker');
  await picker.waitFor({ state: 'visible' });
  await picker.locator('.pool-row__header').first().click();
  await picker.locator('.power-row').first().click();
  const preview = picker.locator('.pool-picker__preview .info-power');
  f = await folds(preview);
  assert.ok(f.length >= 2, 'the preview shows the foldable card');
  for (const s of f) {
    const expected = s.title === 'Effects' ? false : OPEN_BY_DEFAULT.has(s.title);
    assert.equal(s.expanded, expected, `preview ${s.title}: stored fold must reach the picker (${JSON.stringify(f)})`);
  }
  await qa.shot('07-picker-preview-folded');
  await page.keyboard.press('Escape');

  console.log('desktop sprawl check passed');
});
