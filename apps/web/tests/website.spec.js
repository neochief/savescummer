import { test, expect } from '@playwright/test';

function observeErrors(page) {
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('response', response => {
    if (response.status() >= 400) errors.push(`${response.status()} ${response.url()}`);
  });
  return errors;
}

test('the website renders the complete catalog', async ({ page }) => {
  const errors = observeErrors(page);
  await page.goto('/');
  const total = await page.evaluate(() => window.savescummerGameGroups.reduce((n, group) => n + group.games.length, 0));
  await expect(page.locator('.game-card')).toHaveCount(total);
  await expect(page.locator('#benefits article')).toHaveCount(5);
  await expect(page.locator('.downloads > a')).toHaveClass(/btn-3xl/);
  expect(errors).toEqual([]);
});

test('SG UI controls expand the catalog and support keyboard popup dismissal', async ({ page }) => {
  const errors = observeErrors(page);
  await page.goto('/');
  const more = page.getByRole('button', { name: /Show all \d+ games/ });
  await expect(more).toBeVisible();
  await more.click();
  await expect(more).toBeHidden();
  await expect(page.locator('#show-more')).toHaveAttribute('aria-expanded', 'true');
  const hades = page.getByRole('button', { name: /^Hades\./ });
  await hades.focus();
  await page.keyboard.press('Enter');
  await expect(page.locator('#game-tooltip')).toBeVisible();
  await expect(page.locator('#game-tooltip-title')).toHaveText('Hades');
  await expect(page.locator('.game-tooltip-mascot-front')).toBeVisible();
  const storeLink = page.getByRole('link', { name: 'View Hades on Steam' });
  await expect(storeLink).toHaveAttribute('href', 'https://store.steampowered.com/app/1145360/');
  await storeLink.hover();
  await expect(page.locator('#game-tooltip-art')).toHaveClass(/link-hover/);
  await page.keyboard.press('Escape');
  await expect(page.locator('#game-tooltip')).toBeHidden();
  await expect(hades).toBeFocused();
  expect(errors).toEqual([]);
});

for (const width of [320, 390, 768, 1440]) {
  test(`layout and illustrated popup fit a ${width}px viewport`, async ({ page }) => {
    const errors = observeErrors(page);
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto('/');
    await expect(page.locator('#benefits article')).toHaveCount(5);
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await expect(page.locator('footer p')).toHaveText(/© 2026(?:–\d{4})? Alexander Shvets\. All rights reserved\./);
    await expect(page.locator('footer').getByRole('link', { name: 'Contact' })).toHaveAttribute('href', 'mailto:alex@savescummer.app');
    const hades = page.getByRole('button', { name: /^Hades\./ });
    await hades.click();
    await expect(page.locator('.game-tooltip-mascot-front')).toBeVisible();
    await expect.poll(() => page.locator('#game-tooltip').evaluate(element => {
      const rect = element.getBoundingClientRect();
      return rect.left >= 0 && rect.right <= innerWidth && rect.top >= 0 && rect.bottom <= innerHeight;
    })).toBe(true);
    await page.keyboard.press('Escape');
    await expect(page.locator('#game-tooltip')).toBeHidden();
    expect(errors).toEqual([]);
  });
}

test('faded cards cannot be hovered or focused until the gallery is expanded', async ({ page }) => {
  await page.goto('/');
  const more = page.locator('#show-more');
  await expect(more).not.toHaveClass(/btn-flat/);
  await expect(more).toHaveClass(/btn-stone/);
  await expect(more).toHaveClass(/btn-xl/);
  await more.scrollIntoViewIfNeeded();
  expect(await page.locator('#game-groups').evaluate(gallery => getComputedStyle(gallery, '::after').backdropFilter)).toBe('blur(3px)');
  const faded = await page.locator('#game-groups').evaluate(gallery => {
    const bounds = gallery.getBoundingClientRect();
    const visiblePart = parseFloat(getComputedStyle(gallery).getPropertyValue('--gallery-visible-part')) / 100;
    const cutoff = bounds.top + gallery.clientHeight * visiblePart;
    const cards = [...gallery.querySelectorAll('.game-card')];
    const index = cards.findIndex(card => {
      const rect = card.getBoundingClientRect();
      return rect.top >= cutoff && rect.top < bounds.bottom - 4;
    });
    const rect = cards[index].getBoundingClientRect();
    return { index, x: rect.left + rect.width / 2, y: Math.min(rect.top + rect.height / 2, bounds.bottom - 2) };
  });
  const card = page.locator('.game-card').nth(faded.index);
  await expect(card).toHaveJSProperty('inert', true);
  await page.mouse.move(faded.x, faded.y);
  await expect(page.locator('#game-tooltip')).toBeHidden();
  expect(await page.evaluate(({ x, y }) => Boolean(document.elementFromPoint(x, y)?.closest('.game-card')), faded)).toBe(false);
  await more.click();
  expect(await page.locator('#game-groups').evaluate(gallery => getComputedStyle(gallery, '::after').backdropFilter)).toBe('none');
  await expect(card).toHaveJSProperty('inert', false);
  await card.hover();
  await expect(page.locator('#game-tooltip')).toBeVisible();
});

test('cards reveal in gallery order once when they enter the viewport', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await page.addInitScript(() => {
    window.cardReveals = [];
    const animate = Element.prototype.animate;
    Element.prototype.animate = function (...args) {
      const animation = animate.apply(this, args);
      if (this.classList.contains('game-card')) window.cardReveals.push({ card: this, frames: animation.effect.getKeyframes(), timing: animation.effect.getTiming() });
      return animation;
    };
  });
  await page.goto('/');
  await expect(page.locator('.game-card')).toHaveCount(96);
  expect(await page.evaluate(() => window.cardReveals.length)).toBe(0);
  await page.locator('#game-groups').evaluate(gallery => scrollTo({ top: scrollY + gallery.getBoundingClientRect().top - 100, behavior: 'instant' }));
  await expect.poll(() => page.evaluate(() => window.cardReveals.length)).toBeGreaterThanOrEqual(4);
  const firstRow = await page.evaluate(() => window.cardReveals.slice(0, 4).map(({ card, frames, timing }) => ({
    index: [...document.querySelectorAll('.game-card')].indexOf(card),
    delay: timing.delay,
    duration: timing.duration,
    from: { opacity: frames[0].opacity, transform: frames[0].transform },
    to: { opacity: frames[1].opacity, transform: frames[1].transform },
  })));
  expect(firstRow.map(card => card.index)).toEqual([0, 1, 2, 3]);
  expect(firstRow.map(card => card.delay)).toEqual([0, 80, 160, 240]);
  for (const card of firstRow) {
    expect(card.duration).toBe(360);
    expect(card.from).toEqual({ opacity: '0', transform: 'translateY(-14px)' });
    expect(card.to).toEqual({ opacity: '1', transform: 'translateY(0px)' });
  }
  await expect.poll(() => page.locator('.game-card').first().evaluate(card => card.getAnimations().length)).toBe(0);
  await expect(page.locator('.game-card').first()).not.toHaveClass(/reveal-pending/);
  await page.evaluate(() => scrollTo({ top: 0, behavior: 'instant' }));
  await page.locator('#game-groups').evaluate(gallery => scrollTo({ top: scrollY + gallery.getBoundingClientRect().top - 100, behavior: 'instant' }));
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  expect(await page.evaluate(() => window.cardReveals.filter(({ card }) => card === document.querySelector('.game-card')).length)).toBe(1);
});

test('card reveals skip motion when reduced motion is requested', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/');
  await expect(page.locator('.game-card')).toHaveCount(96);
  await page.locator('#game-groups').evaluate(gallery => scrollTo({ top: scrollY + gallery.getBoundingClientRect().top - 100, behavior: 'instant' }));
  const card = page.locator('.game-card').first();
  await expect(card).not.toHaveClass(/reveal-pending/);
  expect(await card.evaluate(card => ({ animations: card.getAnimations().length, opacity: getComputedStyle(card).opacity, transform: getComputedStyle(card).transform }))).toEqual({ animations: 0, opacity: '1', transform: 'none' });
});

for (const width of [390, 1440]) {
  test(`game artwork loads near the viewport rather than all at once at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    const requests = new Set();
    const errors = observeErrors(page);
    page.on('request', request => {
      const path = new URL(request.url()).pathname;
      if (path.startsWith('/games/')) requests.add(path);
    });
    await page.goto('/');
    await expect(page.locator('.game-card')).toHaveCount(96);
    await page.evaluate(() => document.fonts.ready);
    await page.waitForLoadState('networkidle');
    expect(requests.size).toBe(0);

    const images = page.locator('.game-card img');
    const totalImages = await images.count();
    await page.locator('#game-groups').evaluate(gallery => {
      scrollTo({ top: scrollY + gallery.getBoundingClientRect().top - 100, behavior: 'instant' });
    });
    await expect.poll(() => page.locator('.game-card img[src]').count()).toBeGreaterThan(0);
    await page.waitForLoadState('networkidle');
    const previewImages = await page.locator('.game-card img[src]').count();
    expect(requests.size).toBeLessThan(totalImages);
    const lastCard = page.locator('.game-card').last();
    await expect(lastCard.locator('img').first()).not.toHaveAttribute('src');

    await page.locator('#show-more').evaluate(button => button.click());
    await expect.poll(() => page.locator('.game-card img[src]').count()).toBeGreaterThan(previewImages);
    await page.waitForLoadState('networkidle');
    expect(requests.size).toBeLessThan(totalImages);
    await expect(lastCard.locator('img').first()).not.toHaveAttribute('src');

    // Keyboard activation must load artwork even before a deferred card is scrolled into view.
    await lastCard.locator('.game-card-trigger').evaluate(trigger => trigger.focus({ preventScroll: true }));
    await expect(page.locator('#game-tooltip')).toBeVisible();
    await expect.poll(() => lastCard.locator('img').evaluateAll(imgs => imgs.every(img => img.complete && img.naturalWidth > 0))).toBe(true);
    await expect.poll(() => page.locator('#game-tooltip-art img').evaluateAll(imgs => imgs.length > 0 && imgs.every(img => img.complete && img.naturalWidth > 0))).toBe(true);
    await page.keyboard.press('Escape');

    const nearbyCard = page.locator('.game-card').nth(90);
    await expect(nearbyCard.locator('img').first()).not.toHaveAttribute('src');
    await nearbyCard.scrollIntoViewIfNeeded();
    await expect.poll(() => nearbyCard.locator('img').evaluateAll(imgs => imgs.every(img => img.complete && img.naturalWidth > 0))).toBe(true);
    expect(errors).toEqual([]);
  });
}

for (const flipped of [false, true]) {
  test(`Hades popups keep the ordinary card overlap ${flipped ? 'above' : 'below'} the card`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto('/');
    await page.locator('#show-more').click();
    let ordinaryOverlap;
    for (const name of ['FTL: Faster Than Light', 'Hades', 'Hades II']) {
      const trigger = page.getByRole('button', { name: `${name}.` });
      await trigger.evaluate((element, targetTop) => {
        scrollTo({ top: scrollY + element.getBoundingClientRect().top - targetTop, behavior: 'instant' });
      }, flipped ? 800 : 80);
      await trigger.click();
      const popup = page.locator('#game-tooltip');
      await expect(popup).toBeVisible();
      if (name !== 'FTL: Faster Than Light') await expect(page.locator('.game-tooltip-mascot-front')).toBeVisible();
      expect(await popup.evaluate(element => element.classList.contains('flip'))).toBe(flipped);
      const overlap = await popup.evaluate((element, flipped) => {
        const art = element.querySelector('.game-tooltip-art');
        const panel = element.querySelector('.game-tooltip-copy');
        const imageRect = art.getBoundingClientRect();
        const panelRect = panel.getBoundingClientRect();
        return {
          distance: flipped ? panelRect.bottom - imageRect.top : imageRect.bottom - panelRect.top,
          artLayer: getComputedStyle(art).zIndex,
          panelLayer: getComputedStyle(panel).zIndex,
        };
      }, flipped);
      if (name === 'FTL: Faster Than Light') ordinaryOverlap = overlap;
      else expect(overlap).toEqual(ordinaryOverlap);
      expect(overlap.distance).toBeGreaterThan(4);
      await page.keyboard.press('Escape');
      await page.mouse.move(0, 0);
    }
  });
}
