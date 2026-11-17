// Hook 收件箱的真实链路：界面创建接入点 → 服务开关 → 真实 HTTP 投递文本与图片 →
// 列表 / 详情 / 受控预览都从 Rust IPC 取数。只使用合成内容与临时 home（由
// e2e/launcher.mjs 或手动隔离环境提供），不碰用户真实数据。
import { browser, expect } from '@wdio/globals';
import { createServer } from 'node:net';
import { join } from 'node:path';

const runAsync = (body) =>
  browser.executeAsync(
    'var done = arguments[arguments.length - 1]; (async () => {' +
      body +
      '})().then(done, (e) => done({ __error: String(e) }));'
  );

const js = (value) => JSON.stringify(value);

const pageText = (selector) =>
  runAsync(`return document.querySelector(${js(selector)})?.textContent.trim() || '';`);

const clickByText = (selector, needle) =>
  runAsync(
    `const el = [...document.querySelectorAll(${js(selector)})].find((node) => node.textContent.includes(${js(needle)}));` +
      'if (!el) return false; el.click(); return true;'
  );

const waitForText = (selector, needle, message) =>
  browser.waitUntil(async () => (await pageText(selector)).includes(needle), {
    timeout: 30_000,
    timeoutMsg: message,
  });

// Jenkins 的凭据流必须走可见控件：不使用 DOM .click() 绕过折叠区或遮罩。
async function clickVisibleText(scope, selector, text) {
  const elements = await scope.$$(selector);
  for (const element of elements) {
    if (!await element.isDisplayed()) continue;
    const label = (await element.getText()).trim();
    if (typeof text === 'string' ? label === text : text.test(label)) {
      await element.waitForClickable({ timeout: 20_000 });
      await element.click();
      return;
    }
  }
  throw new Error('未找到可见控件：' + text);
}

async function findEndpointRow(name) {
  await browser.$('.hook-endpoint').waitForDisplayed({ timeout: 20_000 });
  for (const row of await browser.$$('.hook-endpoint')) {
    if ((await row.getText()).includes(name)) return row;
  }
  throw new Error('接入点行不存在：' + name);
}

const invoke = (command, args) =>
  runAsync(`return await window.__TAURI__.core.invoke(${js(command)}, ${js(args || {})});`);

async function freePort() {
  return new Promise((resolvePort, rejectPort) => {
    const server = createServer();
    server.once('error', rejectPort);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => resolvePort(port));
    });
  });
}

async function postJson(url, token, body) {
  const response = await fetch(url, {
    method: 'POST',
    headers: { Authorization: 'Bearer ' + token, 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  const text = await response.text();
  return { status: response.status, body: text ? JSON.parse(text) : null };
}

describe('Hook 收件箱（真实 IPC + HTTP）', function () {
  this.timeout(180_000);

  const suffix = String(Date.now());
  const textTitle = 'E2E 文本事件 ' + suffix;
  const imageTitle = 'E2E 图片事件 ' + suffix;
  let endpointId = '';
  let token = '';
  let endpointUrl = '';
  let port = 0;
  const png = Buffer.from(
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=',
    'base64'
  );

  before(async () => {
    await browser.waitUntil(
      async () =>
        (await browser.execute(
          () => typeof window.__TAURI__ !== 'undefined' && typeof document !== 'undefined'
        )) === true,
      { timeout: 60_000, timeoutMsg: '等待壳页面与 Tauri 桥加载超时' }
    );
    await browser.execute(() => {
      window.requestAnimationFrame = (callback) => window.setTimeout(() => callback(Date.now()), 16);
      window.cancelAnimationFrame = (id) => window.clearTimeout(id);
    });
    const clicked = await clickByText('.nav-item', 'Hook');
    expect(clicked).toBe(true);
    await waitForText('h1.page-title', 'Hook', '侧栏切到 Hook 后页面标题没有出现');
  });

  it('在界面上创建接入点，令牌只显示一次', async () => {
    expect(await clickByText('.hook-tabs [role="tab"]', '接入点')).toBe(true);
    await browser.waitUntil(
      async () => (await clickByText('.hook-pane button', '新建接入点')) === true,
      { timeout: 20_000, timeoutMsg: '接入点页没有出现「新建接入点」按钮' }
    );

    const filled = await runAsync(
      `const input = document.querySelector('.el-dialog input[placeholder*="Jenkins"]');` +
        'if (!input) return false;' +
        'const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;' +
        'setter.call(input, "E2E 接入点 ' +
        suffix +
        '");' +
        'input.dispatchEvent(new Event("input", { bubbles: true }));' +
        'return true;'
    );
    expect(filled).toBe(true);
    expect(await clickByText('.el-dialog .dialog-actions button', '创建')).toBe(true);

    await browser.waitUntil(
      async () => ((await pageText('.hook-token code')).length === 64),
      { timeout: 30_000, timeoutMsg: '创建后没有出现一次性令牌' }
    );
    token = await pageText('.hook-token code');
    const endpoints = await invoke('hooks_list_endpoints');
    expect(endpoints?.__error).toBeUndefined();
    endpointId = endpoints.find((row) => row.name === 'E2E 接入点 ' + suffix)?.id || '';
    expect(endpointId).not.toBe('');
    expect(await clickByText('.el-dialog .dialog-actions button', '我已保存')).toBe(true);
    await browser.waitUntil(
      async () => (await pageText('.hook-token code')) === '',
      { timeout: 10_000, timeoutMsg: '关闭后令牌仍留在对话框里' }
    );
  });

  it('用真实设置启用接收器，地址不含令牌', async () => {
    port = await freePort();
    const initial = await invoke('hooks_status');
    // 优先物理适配器；Windows 上 vEthernet/WSL 等虚拟网卡会按名字排在前面。
    const virtualInterface = /^(vEthernet|VMware|VirtualBox|Hyper-V|WSL|Tailscale|ZeroTier|utun|tun|tap|docker|br-)/i;
    const candidates = initial.interfaces || [];
    const network = candidates.find((item) => !virtualInterface.test(item.name)) || candidates[0];
    if (!network) throw new Error('Hook LAN 验收需要一个可用私网网卡；当前环境无可用接口，不能视为测试通过');
    const saved = await invoke('hooks_save_config', { enabled: false, port, interfaceName: network.name, bindIp: network.address, netmask: network.netmask, allowedSubnets: [] });
    expect(saved?.__error).toBeUndefined();
    expect(saved.config.port).toBe(port);

    expect(await clickByText('.hook-tabs [role="tab"]', '设置')).toBe(true);
    await browser.waitUntil(
      async () =>
        (await runAsync(
          'return document.querySelector(".hook-setting-row .el-input-number input")?.value || "";'
        )) === String(port),
      { timeout: 20_000, timeoutMsg: '设置页没有读到保存后的端口' }
    );
    const switched = await runAsync(
      'const el = document.querySelector(".hook-setting-row .el-switch"); if (!el) return false; el.click(); return true;'
    );
    expect(switched).toBe(true);
    await browser.waitUntil(
      async () => {
        const status = await invoke('hooks_status');
        return status?.running === true;
      },
      { timeout: 30_000, timeoutMsg: '打开开关后 Hook 接收器没有运行' }
    );
    endpointUrl = 'http://' + network.address + ':' + port + '/hooks/v1/endpoints/' + endpointId;
    expect(endpointUrl).not.toContain(token);
  });

  it('真实 HTTP 投递文本事件后，列表与详情读得到正文', async () => {
    const created = await postJson(endpointUrl + '/events', token, {
      schema_version: '1.0',
      external_id: 'e2e-text-' + suffix,
      event_type: 'e2e.text',
      occurred_at: new Date().toISOString(),
      title: textTitle,
      text: '来自 E2E 的合成正文',
      parameters: { source: 'e2e' },
      commit: true,
    });
    if (created.status !== 201) {
      throw new Error('创建文本事件失败：' + created.status + ' ' + JSON.stringify(created.body));
    }

    expect(await clickByText('.hook-tabs [role="tab"]', '收件箱')).toBe(true);
    await waitForText('.hook-inbox', '存储', '收件箱没有挂载');
    expect(await clickByText('.hook-filters button', '刷新')).toBe(true);
    await waitForText('.hook-list', textTitle, '刷新后列表里没有这条事件');

    expect(await clickByText('.hook-row', textTitle)).toBe(true);
    await browser.waitUntil(
      async () => (await pageText('.hook-text')).includes('来自 E2E 的合成正文'),
      { timeout: 30_000, timeoutMsg: '详情里没有显示正文' }
    );
    const evidenceDir = process.env.DSH_E2E_EVIDENCE_DIR;
    if (evidenceDir) {
      const switchTheme = async (wantDark) => {
        for (let attempt = 0; attempt < 4; attempt += 1) {
          const dark = await browser.execute(() =>
            document.documentElement.classList.contains('dark')
          );
          if (dark === wantDark) break;
          await browser.execute(() => document.querySelector('.sidebar__theme-btn')?.click());
          await browser.pause(600);
        }
        // 等对话框与主题色过渡画完，截图才不是半透明的一帧。
        await browser.pause(800);
      };
      await switchTheme(false);
      await browser.saveScreenshot(join(evidenceDir, 'hook-detail-light.png'));
      await switchTheme(true);
      await browser.saveScreenshot(join(evidenceDir, 'hook-detail-dark.png'));
    }
    expect(await clickByText('.hook-detail-dialog .dialog-actions button', '关闭')).toBe(true);
  });

  it('图片事件经本地 IPC 预览，不依赖网络地址', async () => {
    const created = await postJson(endpointUrl + '/events', token, {
      schema_version: '1.0',
      external_id: 'e2e-image-' + suffix,
      event_type: 'e2e.image',
      occurred_at: new Date().toISOString(),
      title: imageTitle,
      text: '一张合成图片',
      parameters: {},
      commit: false,
      attachments: [
        {
          client_id: 'shot',
          kind: 'image',
          filename: 'e2e.png',
          media_type: 'image/png',
          size_bytes: png.length,
        },
      ],
    });
    if (created.status !== 201) {
      throw new Error('创建图片事件失败：' + created.status + ' ' + JSON.stringify(created.body));
    }
    expect(created.body.state).toBe('receiving');
    const eventId = created.body.event_id;
    const attachmentId = created.body.attachments[0].attachment_id;

    const upload = await fetch(
      endpointUrl + '/events/' + eventId + '/attachments/' + attachmentId,
      {
        method: 'PUT',
        headers: {
          Authorization: 'Bearer ' + token,
          'Content-Type': 'image/png',
          'Content-Length': String(png.length),
        },
        body: png,
      }
    );
    const uploadText = await upload.text();
    if (upload.status !== 200) {
      throw new Error('上传图片失败：' + upload.status + ' ' + uploadText);
    }

    const commit = await fetch(endpointUrl + '/events/' + eventId + '/commit', {
      method: 'POST',
      headers: { Authorization: 'Bearer ' + token, 'Content-Length': '0' },
    });
    expect(commit.status).toBe(200);

    expect(await clickByText('.hook-tabs [role="tab"]', '收件箱')).toBe(true);
    expect(await clickByText('.hook-filters button', '刷新')).toBe(true);
    await waitForText('.hook-list', imageTitle, '刷新后列表里没有图片事件');
    expect(await clickByText('.hook-row', imageTitle)).toBe(true);
    await waitForText('.hook-attachment', 'e2e.png', '详情里没有列出附件');
    expect(await clickByText('.hook-attachment button', '预览')).toBe(true);
    await browser.waitUntil(
      async () =>
        (await runAsync(
          'const img = document.querySelector("img.hook-image"); return !!img && img.src.startsWith("data:image/png;base64,");'
        )) === true,
      { timeout: 30_000, timeoutMsg: '图片预览没有走本地 base64 IPC' }
    );
    expect(await clickByText('.hook-detail-dialog .dialog-actions button', '关闭')).toBe(true);
  });

  it('Jenkins 通知：独立 Basic 密码入箱，reported_result 不被当成已核验', async () => {
    await clickVisibleText(browser, '.hook-tabs [role="tab"]', /^接入点（/);
    const row = await findEndpointRow('E2E 接入点 ' + suffix);
    const more = await row.$('.hook-more');
    await clickVisibleText(more, 'summary', '更多');
    await browser.waitUntil(async () => (await more.getAttribute('open')) !== null, {
      timeout: 10_000, timeoutMsg: '更多操作没有展开',
    });
    await clickVisibleText(more, 'button', '接入 Jenkins');
    await browser.waitUntil(
      async () => (await pageText('.hook-notification-dialog .hook-token code')).length === 64,
      { timeout: 30_000, timeoutMsg: '没有生成 Jenkins 通知密码' }
    );
    const password = await pageText('.hook-notification-dialog .hook-token code');
    const dialog = await browser.$('.hook-notification-dialog');
    expect(await dialog.getText()).toContain(endpointUrl + '/jenkins-notification');
    const copySecret = await dialog.$('button=复制 Jenkins Secret text');
    await expect(copySecret).toBeDisplayed();
    await clickVisibleText(dialog, '.dialog-actions button', '我已保存');
    await browser.waitUntil(async () => (await pageText('.hook-notification-dialog .hook-token code')) === '', {
      timeout: 10_000, timeoutMsg: '关闭后通知密码仍留在界面',
    });

    const number = Date.now() % 1000000;
    // Notification 插件的当前顶层格式；这是合成 HTTP 负载，不冒充真实 Jenkins 实机验收。
    const payload = {
      name: 'build-app',
      display_name: 'Build App',
      url: 'job/folder/job/build-app/',
      build: {
        number,
        phase: 'FINALIZED',
        status: 'SUCCESS',
        timestamp: Date.now(),
        duration: 1000,
      },
    };
    const response = await fetch(
      endpointUrl + '/jenkins-notification',
      {
        method: 'POST',
        headers: {
          Authorization: 'Basic ' + Buffer.from('jenkins:' + password).toString('base64'),
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(payload),
      }
    );
    if (response.status !== 201) {
      throw new Error('通知入箱失败：' + response.status + ' ' + (await response.text()));
    }

    expect(await clickByText('.hook-tabs [role="tab"]', '收件箱')).toBe(true);
    expect(await clickByText('.hook-filters button', '刷新')).toBe(true);
    const title = 'Build App #' + number + ' SUCCESS';
    await waitForText('.hook-list', title, '通知事件没有出现在收件箱');
    expect(await clickByText('.hook-row', title)).toBe(true);
    await browser.waitUntil(
      async () => (await pageText('.hook-text')).includes('"reported_result": "SUCCESS"'),
      { timeout: 30_000, timeoutMsg: '详情里没有 reported_result' }
    );
    expect(await pageText('.hook-detail-dialog')).not.toContain('verified_result');
    expect(await clickByText('.hook-detail-dialog .dialog-actions button', '关闭')).toBe(true);
  });

  it('Jenkins 模板通过可见单选框创建，直接得到通知凭据而非通用 Bearer', async () => {
    await clickVisibleText(browser, '.hook-tabs [role="tab"]', /^接入点（/);
    await clickVisibleText(browser, '.hook-pane button', '新建接入点');
    const createDialog = await browser.$('.el-dialog input[placeholder*="Jenkins"]');
    await createDialog.waitForDisplayed({ timeout: 10_000 });
    const name = 'E2E Jenkins 模板 ' + suffix;
    await createDialog.setValue(name);
    await clickVisibleText(browser, '.el-dialog .el-radio', 'Jenkins');
    await clickVisibleText(browser, '.el-dialog .dialog-actions button', '创建');
    await browser.waitUntil(async () => (await pageText('.hook-notification-dialog .hook-token code')).length === 64, {
      timeout: 30_000, timeoutMsg: 'Jenkins 模板未直接生成通知凭据',
    });
    const endpoints = await invoke('hooks_list_endpoints');
    const endpoint = endpoints.find(item => item.name === name);
    expect(endpoint?.notification_enabled).toBe(true);
    const dialog = await browser.$('.hook-notification-dialog');
    const address = new URL(endpointUrl);
    address.pathname = '/hooks/v1/endpoints/' + endpoint.id + '/jenkins-notification';
    expect(await dialog.getText()).toContain(address.href);
    const password = await pageText('.hook-notification-dialog .hook-token code');
    await clickVisibleText(dialog, '.dialog-actions button', '我已保存');
    await browser.waitUntil(async () => (await pageText('.hook-notification-dialog .hook-token code')) === '', {
      timeout: 10_000, timeoutMsg: 'Jenkins 模板凭据关闭后未清除',
    });
    const row = await findEndpointRow(name);
    expect(await row.getText()).not.toContain('PowerShell 测试');
    expect(await row.getText()).not.toContain('Bash 测试');
    expect(await row.getText()).not.toContain(password);
    expect(await row.getText()).toContain('轮换通知凭据');
  });

  it('无效的未保存网段不阻止关闭服务，草稿可明确恢复', async () => {
    const before = await invoke('hooks_status');
    expect(before.running).toBe(true);
    await clickVisibleText(browser, '.hook-tabs [role="tab"]', '设置');
    const advanced = await browser.$('.hook-advanced');
    await advanced.waitForDisplayed({ timeout: 10_000 });
    if ((await advanced.getAttribute('open')) === null) {
      await clickVisibleText(advanced, 'summary', '高级网络设置');
    }
    const subnets = await advanced.$('textarea');
    await subnets.waitForDisplayed({ timeout: 10_000 });
    await subnets.setValue('10.20.0.0/33');
    await waitForText('.hook-setting-draft', '草稿未保存', '网段编辑未标记为未保存');
    const toggle = await browser.$('.hook-setting-row .el-switch');
    await toggle.waitForClickable({ timeout: 10_000 });
    await toggle.click();
    await browser.waitUntil(async () => {
      const status = await invoke('hooks_status');
      return status?.running === false && status?.config?.enabled === false;
    }, { timeout: 30_000, timeoutMsg: '无效网段草稿阻止了关闭服务' });
    const after = await invoke('hooks_status');
    expect(after.config.allowed_subnets).toEqual(before.config.allowed_subnets);
    expect(after.config.bind_ip).toBe(before.config.bind_ip);
    expect(after.config.port).toBe(before.config.port);
    expect(await subnets.getValue()).toBe('10.20.0.0/33');
    await clickVisibleText(browser, '.hook-setting-draft button', '恢复已保存');
    await browser.waitUntil(async () => (await subnets.getValue()) === before.config.allowed_subnets.join('\n'), {
      timeout: 10_000, timeoutMsg: '恢复按钮没有还原已保存网段',
    });
  });

});
