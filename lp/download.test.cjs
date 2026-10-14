const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const source = fs.readFileSync(__dirname + '/public/download.js', 'utf8');
const download = 'https://github.com/ttizze/remote-agent/releases/download/prd-75/bex-windows-x86_64-release.zip';

async function renderWindows({ language = 'en', platform = 'Win32', assets = [], ok = true, fail = false } = {}) {
  const element = () => ({ classList: { toggle() {} }, innerHTML: '', href: '' });
  const primary = [element(), element()];
  const grid = [element()];
  const menu = [element()];
  const context = {
    window: {}, URL, AbortSignal,
    navigator: { platform, userAgent: platform === 'Win32' ? 'Windows NT 10.0' : 'Macintosh' },
    document: {
      documentElement: { lang: language, classList: { toggle() {} } },
      querySelectorAll(selector) {
        return ({ '[data-dl-primary]': primary, '[data-dl-grid]': grid, '[data-dl-menu]': menu })[selector] || [];
      },
      addEventListener() {}
    },
    MutationObserver: class { observe() {} },
    fetch: async () => {
      if (fail) throw new Error('offline');
      return { ok, json: async () => ({ assets }) };
    }
  };
  vm.runInNewContext(source, context);
  await new Promise(resolve => setImmediate(resolve));
  return { primary, grid: grid[0].innerHTML, menu: menu[0].innerHTML };
}

for (const language of ['en', 'ja']) {
  test(`Windows ${language} hero, menu and card download the uploaded ZIP directly`, async () => {
    const result = await renderWindows({ language, assets: [{ name: 'bex-windows-x86_64-release.zip', state: 'uploaded', browser_download_url: download }] });
    for (const button of result.primary) {
      assert.equal(button.href, download);
      assert.match(button.innerHTML, language === 'ja' ? /Windows版をダウンロード/ : /Download for Windows/);
    }
    assert.ok(result.grid.includes(`href="${download}"`));
    const mac = await renderWindows({ language, platform: 'MacIntel', assets: [{ name: 'bex-windows-x86_64-release.zip', state: 'uploaded', browser_download_url: download }] });
    assert.ok(mac.menu.includes(`href="${download}"`));
    assert.ok(!result.primary[0].innerHTML.includes('coming soon'));
  });
}
for (const [name, options] of Object.entries({
  'Mac-only release': {},
  'failed API': { ok: false },
  offline: { fail: true },
  'unfinished upload': { assets: [{ name: 'bex-windows-x86_64-release.zip', state: 'new', browser_download_url: download }] },
  'foreign download': { assets: [{ name: 'bex-windows-x86_64-release.zip', state: 'uploaded', browser_download_url: 'https://example.com/bex.zip' }] }
})) {
  test(`${name} retains Windows setup without a broken ZIP link`, async () => {
    const result = await renderWindows(options);
    assert.match(result.primary[0].href, /docs\/WINDOWS.md$/);
    assert.match(result.primary[0].innerHTML, /Use on Windows/);
    assert.ok(!result.primary[0].innerHTML.includes('coming soon'));
  });
}
