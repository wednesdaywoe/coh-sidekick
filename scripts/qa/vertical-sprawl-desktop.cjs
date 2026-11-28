#!/usr/bin/env node
/**
 * QA check for the planner's vertical layout, desktop (vertical-sprawl items 3, 4, 6, 7;
 * accolades-and-pool-headers items 1, 2).
 *
 * Needs serve.cjs running on 8787 (see the qa-check skill). Run:
 *   node scripts/qa/vertical-sprawl-desktop.cjs
 *
 * What it walks, in order:
 *  1. the Incarnates panel carries the sockets and nothing else, Available shows neither
 *     strip, the panel hides and returns via the Display menu, and the Accolades row rides
 *     the foot of Build Settings — its picker opens and a toggle moves the row's summary
 *     (and flipping back leaves the build as found);
 *  2. the Pools / Epic triggers are one row tall at minimum width, and the set headers hold
 *     one ellipsized line with the full name in the hover title;
 *  3. a fresh profile opens the Info card with Damage, Effects and Cost and timing and
 *     nothing else, folding one section survives a reload, follows to another power, to the
 *     Details tab and into the pool picker's preview, and the heading answers to Enter.
 */

const assert = require('node:assert/strict');
const { run } = require('../../.claude/skills/qa-check/scripts/session.cjs');

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

  // ---- 1. sockets alone on their panel; Accolades rides Build Settings --------------
  await incarnates.scrollIntoViewIfNeeded();
  const incText = await incarnates.evaluate((e) => e.textContent);
  assert.ok(incText.includes('Incarnates'), 'the panel is titled Incarnates');
  assert.ok(!incText.includes('Accolades'), 'the accolade strip has left the panel');
  assert.ok(await incarnates.locator('.incarnate-socket').count() > 0,
    'the socket row still rides the panel');
  const availText = await available.evaluate((e) => e.textContent);
  assert.ok(!availText.includes('Accolades'), 'Available must no longer draw the accolade strip');
  assert.ok(!/socket/i.test(availText), 'Available must no longer draw incarnate sockets');
  // The quickbar pill for the panel (its restore route alongside the Display menu).
  console.log('quickbar pills matching "Incarnates":',
    await page.locator('.quickbar-pill', { hasText: 'Incarnates' }).count());
  await qa.shot('01-incarnates-panel', incarnates);

  // Hide it from its header, put it back from the Display menu — the routes a panel that
  // ships on the grid has (no quickbar pill unless the user pins one).
  await page.getByRole('button', { name: 'Hide Incarnates panel' }).click();
  assert.equal(await incarnates.isVisible(), false, 'hidden means off the grid');
  const display = page.getByRole('button', { name: /Display/ });
  await display.click();
  await page
    .locator('.quickbar-menu button[title="Put Incarnates back on the grid"]')
    .first()
    .click();
  await incarnates.waitFor({ state: 'visible' });
  await page.keyboard.press('Escape');
  await qa.shot('02-panel-restored-from-display-menu', incarnates);

  // Build Settings carries the Accolades row, at the foot of the combat form. The trigger
  // is found by its label; the drift text would rename it for a drifted build, and this
  // fixture is not one.
  const bsTrigger = page
    .locator('.popover-trigger')
    .filter({ has: page.locator('.popover-label', { hasText: 'Build Settings' }) });
  await bsTrigger.click();
  const bs = page.locator('.popover-panel[aria-label="Build Settings"]');
  await bs.waitFor({ state: 'visible' });
  const strip = bs.locator('.picker-strip');
  assert.equal(await strip.count(), 1, 'an Accolades row rides the foot of Build Settings');
  assert.match(await strip.innerText(), /accolades/i);
  const summaryOf = async () => (await strip.locator('.picker-strip__power').innerText()).trim();
  const summary0 = await summaryOf();
  assert.match(summary0, /^(none|\d+ of \d+)$/, `row carries the enabled summary, got "${summary0}"`);
  // The last control in the form — the combat inputs lead, the accolades are the rare dip.
  const bodyKids = await bs.locator('.combat > *').evaluateAll((els) =>
    els.map((e) => e.className));
  assert.match(String(bodyKids.at(-1)), /picker-strip/,
    `the strip is last in the form, got ${JSON.stringify(bodyKids)}`);
  await qa.shot('03-build-settings-accolades');

  // The row opens the picker and closes its host surface on the way — the tool launcher's
  // rule, so no popover stands over the modal it raised. The picker is shell-level state
  // and outlives the popover. A toggle moves the count, and flipping back leaves the
  // build exactly as it found it.
  await strip.locator('.picker-strip__open').click();
  const accPicker = page.locator('.floating-panel[aria-label="Accolades"]');
  await accPicker.waitFor({ state: 'visible' });
  await bs.waitFor({ state: 'hidden' });
  assert.equal(await accPicker.isVisible(), true, 'the picker is build state, not popover state');
  assert.ok(await accPicker.locator('.accolade-row').count() > 0, 'the picker lists the toggles');
  const countOf = async () => (await accPicker.locator('.accolade-picker__count').innerText()).trim();
  const count0 = await countOf();
  const row = accPicker.locator('.accolade-row').first();
  await row.click();
  const count1 = await countOf();
  assert.notEqual(count1, count0, `the toggle updates the enabled count (${count0} -> ${count1})`);
  await qa.shot('04-accolades-picker-open');
  await row.click();
  assert.equal(await countOf(), count0, 'flipping back restores the count');
  await accPicker.locator('.modal-close').click();
  await accPicker.waitFor({ state: 'hidden' });

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
  await qa.shot('05-set-adders-default');

  // Drag the panel narrower with its bottom-edge handle. Wheel back down first: the
  // scroll-up that freed the header for shot 05 pushed the panel's bottom — and its
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

  // Headers hold one line however narrow: nowrap, one line box, the full name in the
  // hover title, and the ellipsis machinery armed (overflow + text-overflow — with them
  // set, a name too long for the track truncates by the browser's own rule; which build
  // name trips it is fixture luck, so truncation itself is reported, not asserted).
  const nameInfo = await pools.locator('.powerset-group__name').evaluateAll((els) =>
    els.map((e) => {
      const cs = getComputedStyle(e);
      return {
        text: e.textContent.trim(),
        title: e.getAttribute('title'),
        whiteSpace: cs.whiteSpace,
        overflow: cs.overflowX,
        textOverflow: cs.textOverflow,
        boxH: e.clientHeight,
        oneLine: e.scrollHeight <= e.clientHeight + 1,
        truncated: e.scrollWidth > e.clientWidth + 1,
      };
    }));
  assert.ok(nameInfo.length > 0, 'the pools panel shows set headers');
  for (const n of nameInfo) {
    assert.equal(n.whiteSpace, 'nowrap', `"${n.text}" must not wrap`);
    assert.equal(n.overflow, 'hidden', `"${n.text}" must clip rather than spill`);
    assert.equal(n.textOverflow, 'ellipsis', `"${n.text}" must carry an ellipsis`);
    assert.ok(n.oneLine && n.boxH <= 20,
      `"${n.text}" must hold one line, got box ${n.boxH}px scroll ${JSON.stringify(n)}`);
    assert.equal(n.title, n.text, `"${n.text}" hover title carries the full name`);
  }
  console.log('headers at minimum width:',
    JSON.stringify(nameInfo.map((n) => `${n.text}${n.truncated ? ' …' : ''}`)));
  await pools.scrollIntoViewIfNeeded();
  await page.mouse.wheel(0, -220);
  await page.waitForTimeout(150);
  await qa.shot('06-set-adders-minimum-width');

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
  // Description and Adjusters ride the same rail: folded on a fresh profile, and nothing
  // of theirs is stored yet — their keys live under `card:`, outside the two tabs.
  for (const title of ['Description', 'Adjusters']) {
    const s = f.find((x) => x.title === title);
    assert.ok(s, `Soul Drain's card carries a ${title} section, got ${JSON.stringify(f)}`);
    assert.equal(s.expanded, false, `${title} starts folded on a fresh profile`);
  }
  await qa.shot('07-info-default-folds');

  // Opening each writes its card: key; the Adjusters' controls come back with the fold.
  await card.locator('h4.is-foldable', { hasText: 'Adjusters' }).click();
  assert.ok(await card.locator('.adjuster-row, .adjuster-choice').first().isVisible(),
    'the adjuster inputs return with the fold');
  await card.locator('h4.is-foldable', { hasText: 'Description' }).click();
  assert.ok(await card.locator('.info-description p, .short-help').first().isVisible(),
    'the prose returns with the fold');
  const cardKeys = await page.evaluate(() => localStorage.getItem('sk-info-folds'));
  assert.match(cardKeys, /"card:description":false/, `the open description persisted, got ${cardKeys}`);
  assert.match(cardKeys, /"card:adjusters":false/, `the open adjusters persisted, got ${cardKeys}`);
  await qa.shot('07b-description-adjusters-open');

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
  await qa.shot('08-effects-folded');

  // Details tab: everything starts folded; opening Accepts is a preference too. The sweep
  // is pane-scoped on purpose — Description and Adjusters live OUTSIDE the pane (true on
  // every face) and are open here, by this walk's own doing; only the pane's own sections
  // claim "starts folded" when the tab opens.
  await info.getByRole('tab', { name: 'Details', exact: true }).click();
  f = await folds(card.locator('.info-pane'));
  for (const s of f) assert.equal(s.expanded, false, `Details starts folded, ${s.title} was open`);
  const cardLevel = await folds(card.locator(':scope > .info-block'));
  for (const s of cardLevel) {
    assert.equal(s.expanded, true, `${s.title} is true on every face — it stayed open across the tab switch`);
  }
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

  // The card-level opens survived the reload too, on a power this walk never touched:
  // Cloak of Fear carries both headings, and both come up open from storage.
  await available.locator('.power-row', { hasText: 'Cloak of Fear' }).first().hover();
  f = await folds(card);
  assert.equal(f.find((s) => s.title === 'Description').expanded, true,
    'the open description follows to another power, across a reload');
  assert.equal(f.find((s) => s.title === 'Adjusters').expanded, true,
    'the open adjusters follow too');
  assert.ok(await card.locator('.adjuster-row, .adjuster-choice').first().isVisible(),
    'and the inputs are really there, not just the aria state');

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
    // Effects: the walk folded it. Description: the walk OPENED it, and the `card:` key
    // is storage like any other — the preview must honour it too. Everything else is on
    // the authored default. (Adjusters never appears here: the picker passes no adjusters.)
    const expected = s.title === 'Effects' ? false
      : s.title === 'Description' ? true
      : OPEN_BY_DEFAULT.has(s.title);
    assert.equal(s.expanded, expected, `preview ${s.title}: stored fold must reach the picker (${JSON.stringify(f)})`);
  }
  await qa.shot('09-picker-preview-folded');
  await page.keyboard.press('Escape');

  console.log('desktop sprawl check passed');
});
