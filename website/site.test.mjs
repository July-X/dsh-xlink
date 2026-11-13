import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { runInNewContext } from 'node:vm';

test('theme comparison updates the split and accessible value', () => {
  const values = {};
  const range = {
    value: '50',
    setAttribute(name, value) { values[name] = value; },
    addEventListener(name, handler) { values[name] = handler; },
  };
  const compare = {
    querySelector: () => range,
    style: { setProperty(name, value) { values[name] = value; } },
  };
  const document = { querySelectorAll: (selector) => selector === ".compare" ? [compare] : [], querySelector: () => null };
  runInNewContext(readFileSync(new URL('./site.js', import.meta.url), 'utf8'), { document });
  assert.equal(values['--split'], '50%');
  for (const light of [0, 37, 100]) {
    range.value = String(light);
    values.input();
    assert.equal(values['--split'], `${light}%`);
    assert.equal(values['aria-valuetext'], `浅色界面 ${light}%，深色界面 ${100 - light}%`);
  }
});

const source = readFileSync(new URL('./site.js', import.meta.url), 'utf8');
const element = (extra = {}) => ({
  handlers: {}, attributes: {}, dataset: {},
  classList: { add() {}, remove() {} },
  addEventListener(name, handler) { this.handlers[name] = handler; },
  setAttribute(name, value) { this.attributes[name] = value; },
  ...extra,
});

test('screen theme updates image, accessible state and enlarged image together', () => {
  const image = element({ dataset: { light: 'light.webp', dark: 'dark.webp' } });
  const link = element();
  const buttons = ['light', 'dark'].map((screenTheme) => element({ dataset: { screenTheme } }));
  const screen = element({
    querySelector: (selector) => ({ '[data-screen-image]': image, '[data-screen-link]': link, '[data-screen-title]': { textContent: '插件管理' } })[selector],
    querySelectorAll: () => buttons,
  });
  const document = { querySelectorAll: (selector) => selector === '[data-screen]' ? [screen] : [], querySelector: () => null };
  runInNewContext(source, { document });
  for (const [index, theme, label] of [[1, 'dark', '深色'], [0, 'light', '浅色'], [1, 'dark', '深色']]) {
    buttons[index].handlers.click();
    assert.equal(image.src, `${theme}.webp`);
    assert.equal(link.href, image.src);
    assert.match(image.alt, new RegExp(label));
    assert.equal(link.dataset.caption, `插件管理 · ${label} · 示例数据`);
    assert.equal(buttons[index].attributes['aria-pressed'], 'true');
    assert.equal(buttons[1 - index].attributes['aria-pressed'], 'false');
  }
});

test('image viewer preserves modified links, closes and restores focus, and reopens', () => {
  const image = element();
  const caption = element();
  const original = element();
  const close = element();
  let focused = 0;
  const link = element({ href: 'plugins-dark.webp', dataset: { caption: '插件管理 · 深色 · 示例数据' }, focus() { focused++; } });
  const classes = new Set();
  const viewer = element({
    open: false,
    showModal() { this.open = true; },
    close() { this.open = false; this.handlers.close(); },
    querySelector: (selector) => ({ img: image, '#image-viewer-caption': caption, '[data-viewer-original]': original, '[data-viewer-close]': close })[selector],
  });
  const document = {
    querySelector: () => viewer,
    querySelectorAll: (selector) => selector === '[data-lightbox]' ? [link] : [],
    documentElement: { classList: { add: (v) => classes.add(v), remove: (v) => classes.delete(v) } },
  };
  runInNewContext(source, { document });
  link.handlers.click({ ctrlKey: true, preventDefault() { assert.fail('modified clicks must use native navigation'); } });
  assert.equal(viewer.open, false);
  let prevented = 0;
  const click = () => link.handlers.click({ button: 0, preventDefault() { prevented++; } });
  click();
  assert.equal(viewer.open, true);
  assert.equal(image.src, link.href);
  assert.equal(original.href, link.href);
  assert.equal(caption.textContent, link.dataset.caption);
  assert.ok(classes.has('viewer-open'));
  viewer.handlers.click({ target: image });
  assert.equal(viewer.open, true);
  close.handlers.click();
  assert.equal(viewer.open, false);
  assert.equal(focused, 1);
  assert.equal(classes.size, 0);
  click();
  viewer.handlers.click({ target: viewer });
  assert.equal(viewer.open, false);
  assert.equal(focused, 2);
  assert.equal(prevented, 2);
});

test('pages offer original-image fallbacks, labeled themes and dialog close controls', () => {
  for (const file of ['index.html', 'features.html']) {
    const html = readFileSync(new URL(file, import.meta.url), 'utf8');
    assert.match(html, /<script src="site.js" defer><\/script>/);
    assert.match(html, /<dialog[^>]+aria-labelledby="image-viewer-caption"/);
    assert.match(html, /data-viewer-close autofocus aria-label="关闭图片预览"/);
    for (const name of ['plugins', 'memory']) {
      assert.match(html, new RegExp(`href="assets/${name}-light.webp"`));
      assert.match(html, new RegExp(`data-dark="assets/${name}-dark.webp"`));
    }
    assert.match(html, /aria-pressed="true"/);
    assert.match(html, /示例数据/);
  }
});

test('unsupported dialog leaves ordinary image links untouched', () => {
  let intercepted = false;
  const document = {
    querySelector: () => ({}),
    querySelectorAll: (selector) => selector === '[data-lightbox]' ? [{ addEventListener() { intercepted = true; } }] : [],
  };
  runInNewContext(source, { document });
  assert.equal(intercepted, false);
});
