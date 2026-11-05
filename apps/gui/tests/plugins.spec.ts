import { test, expect } from '@playwright/test';
import { createServer } from 'node:http';
import { mkdtemp, readFile, readdir, rm, writeFile, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { spawn, type ChildProcess } from 'node:child_process';
import { pluginServices } from './fixtures/plugin-services';

let core: ChildProcess;
let directory: string;
let endpoint: string;
let address: string;
let services: Awaited<ReturnType<typeof pluginServices>>;
test.beforeEach(async () => {
  for (const id of ['slack', 'github', 'notion', 'figma']) await fetch(endpoint + '/plugins/' + id, { method: 'DELETE' });
  services.requests.length = 0;
});
async function startCore() {
  core = spawn(resolve('../../target/debug/omnistream'), ['serve', address], { cwd: directory, env: { ...process.env, OMNISTREAM_HOME: directory, OMNISTREAM_PLUGIN_TEST_ORIGIN: services.origin }, stdio: 'ignore' });
  await expect.poll(() => fetch(endpoint + '/health').then(response => response.ok).catch(() => false)).toBe(true);
}
async function stopCore() {
  if (core && core.exitCode === null) { core.kill(); await new Promise(done => core.once('exit', done)); }
}
test.beforeAll(async () => {
  directory = await mkdtemp(join(tmpdir(), 'plugins-e2e-'));
  services = await pluginServices();
  await writeFile(join(directory, 'config.toml'), ['model_provider="fixture"', 'model="fixture"', '[model_providers.fixture]', 'name="Fixture"', 'base_url="' + services.origin + '/model"', 'models=[{id="fixture"}]', '[model_providers.fixture.auth]', 'command="printf"', 'args=["fixture-model-key"]'].join('\n'));
  const listener = createServer();
  await new Promise<void>(done => listener.listen(0, '127.0.0.1', done));
  address = '127.0.0.1:' + (listener.address() as { port: number }).port;
  await new Promise<void>(done => listener.close(() => done()));
  endpoint = 'http://' + address;
  await startCore();
});

test('connect all four services, invoke reads and writes, and enforce chat approvals', async ({ page }, testInfo) => {
  test.setTimeout(90000);
  for (const id of ['slack', 'github', 'notion', 'figma']) await fetch(endpoint + '/plugins/' + id + '/install', { method: 'POST' });
  await page.addInitScript(url => { localStorage.setItem('omnistream.url', url); localStorage.setItem('omnistream.locale', 'zh-CN'); }, endpoint);
  await page.goto('/chat/plugins-live');
  await page.getByRole('button', { name: '插件市场', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '插件市场' });
  for (const [id, name] of [['slack', 'Slack'], ['github', 'GitHub'], ['notion', 'Notion'], ['figma', 'Figma']]) {
    await dialog.getByRole('article', { name, exact: true }).getByRole('button', { name: '配置与使用' }).click();
    await dialog.getByLabel('访问令牌', { exact: true }).fill('fixture-' + id + '-token');
    await dialog.getByLabel('允许写入此应用').check();
    await dialog.getByRole('button', { name: '验证并保存' }).click();
    await expect(dialog.getByText('连接配置已保存')).toBeVisible();
    await expect(dialog.getByLabel('访问令牌', { exact: true })).toHaveValue('');
    if (id === 'slack') {
      await dialog.getByRole('button', { name: '列出会话', exact: true }).click();
      await expect(dialog.getByText('general', { exact: true })).toBeVisible();
      await dialog.getByRole('button', { name: '下一页' }).click();
      await expect(dialog.getByRole('button', { name: '下一页' })).toHaveCount(0);
      await dialog.getByLabel('选择操作').selectOption('send_message');
      await dialog.getByLabel(/会话 ID/).fill('C123');
      await dialog.getByLabel(/消息内容/).fill('Hello from OmniStream');
      await dialog.getByRole('button', { name: '发送消息', exact: true }).click();
    } else if (id === 'github') {
      await dialog.getByRole('button', { name: '列出仓库', exact: true }).click();
      await expect(dialog.getByRole('link', { name: 'Fixture repository' })).toBeVisible();
      const download = page.waitForEvent('download');
      await dialog.getByRole('button', { name: '导出 JSON' }).click();
      await (await download).saveAs(testInfo.outputPath('github-results.json'));
      await dialog.getByLabel('选择操作').selectOption('create_issue');
      await dialog.getByLabel(/仓库所有者/).fill('org'); await dialog.getByLabel(/仓库名称/).fill('project');
      await dialog.getByLabel(/标题/).fill('GUI issue'); await dialog.getByLabel('正文', { exact: true }).fill('Created from the marketplace');
      await dialog.getByRole('button', { name: '创建 Issue', exact: true }).click();
    } else if (id === 'notion') {
      await dialog.getByRole('button', { name: '搜索页面', exact: true }).click();
      await expect(dialog.getByRole('link', { name: 'Team notes' })).toBeVisible();
      await dialog.getByLabel('选择操作').selectOption('create_page');
      await dialog.getByLabel(/父页面 ID/).fill('page-1'); await dialog.getByLabel(/标题/).fill('OmniStream notes');
      await dialog.getByLabel('页面正文').fill('Created from the marketplace');
      await dialog.getByRole('button', { name: '创建子页面', exact: true }).click();
    } else {
      await dialog.getByLabel(/文件 Key/).fill('File123');
      await dialog.getByRole('button', { name: '读取设计文件', exact: true }).click();
      await expect(dialog.getByText('Fixture design', { exact: true })).toBeVisible();
      await dialog.getByLabel('选择操作').selectOption('post_comment');
      await dialog.getByLabel(/文件 Key/).fill('File123'); await dialog.getByLabel(/评论内容/).fill('Review from OmniStream');
      await dialog.getByRole('button', { name: '发表评论', exact: true }).click();
    }
    await expect(dialog.getByText('操作成功', { exact: true })).toBeVisible();
    expect((await stat(join(directory, 'plugins/connections/' + id + '.json'))).mode & 0o777).toBe(0o600);
    await dialog.getByRole('button', { name: '返回插件市场' }).click();
  }
  const publicData = await fetch(endpoint + '/plugins/marketplace').then(response => response.text());
  for (const id of ['slack', 'github', 'notion', 'figma']) expect(publicData).not.toContain('fixture-' + id + '-token');
  expect(services.requests.find(request => request.path === '/slack/chat.postMessage')?.body.text).toBe('Hello from OmniStream');
  expect(services.requests.find(request => request.path === '/notion/pages')?.body.parent.page_id).toBe('page-1');
  expect(services.requests.find(request => request.path === '/figma/files/File123/comments' && request.method === 'POST')?.body.client_meta).toEqual({ x: 0, y: 0 });
  await stopCore(); await startCore(); await page.reload();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole('button', { name: '切换侧栏' }).click(); await page.getByRole('button', { name: '插件市场', exact: true }).click();
  await expect(dialog.locator('.plugin-connection-status.connected')).toHaveCount(4);
  await dialog.getByRole('article', { name: 'GitHub', exact: true }).getByRole('button', { name: '配置与使用' }).click();
  await expect(dialog.getByText('当前账号：octocat')).toBeVisible();
  await dialog.getByLabel('访问令牌', { exact: true }).fill('bad-token');
  await dialog.getByRole('button', { name: '验证并保存' }).click();
  await expect(dialog.getByRole('alert')).toContainText('HTTP 401');
  expect((await readFile(join(directory, 'plugins/connections/github.json'), 'utf8'))).toContain('fixture-github-token');
  await dialog.getByLabel('访问令牌', { exact: true }).fill('');
  await dialog.getByLabel('启用此插件').uncheck();
  await dialog.getByRole('button', { name: '保存', exact: true }).click();
  await expect(dialog.getByText('已停用', { exact: true })).toBeVisible();
  const denied = await fetch(endpoint + '/plugins/github/invoke', { method: 'POST', body: JSON.stringify({ action: 'list_repositories', input: {} }) }); expect(denied.status).toBe(409);
  await dialog.getByLabel('启用此插件').check(); await dialog.getByRole('button', { name: '验证并保存' }).click();
  await expect(dialog.getByText('已连接', { exact: true })).toBeVisible();
  await dialog.getByRole('button', { name: '测试连接' }).click(); await expect(dialog.getByText('连接验证成功')).toBeVisible();
  await dialog.getByRole('button', { name: '列出仓库', exact: true }).click();
  await expect(dialog.getByRole('link', { name: 'Fixture repository' })).toBeVisible();
  expect(await dialog.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  await dialog.screenshot({ path: testInfo.outputPath('plugin-connected-mobile.png') });
  await page.keyboard.press('Escape');

  const writeCount = () => services.requests.filter(request => request.path === '/github/repos/org/project/issues' && request.method === 'POST').length;
  const initialWrites = writeCount();
  await page.getByLabel('消息', { exact: true }).fill('读取 GitHub Issue'); await page.getByLabel('消息', { exact: true }).press('Enter');
  await expect(page.locator('.chat-message.assistant').getByText('插件读取已完成。', { exact: true })).toBeVisible();
  expect(services.requests.some(request => request.path === '/github/repos/org/project/issues' && request.method === 'GET')).toBe(true);
  await page.getByLabel('消息', { exact: true }).fill('创建 GitHub Issue'); await page.getByLabel('消息', { exact: true }).press('Enter');
  const pending = page.locator('.pending-request');
  await expect(pending).toContainText('插件写入审批'); await expect(pending).toContainText('Assistant issue');
  await expect(pending.getByText('标题', { exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  expect(await fetch(endpoint + '/conversation/detail', { method: 'POST', body: JSON.stringify({ conversation: 'isolated-chat' }) }).then(response => response.json()).then(detail => detail.approvals)).toEqual([]);
  expect(writeCount()).toBe(initialWrites);
  expect((await fetch(endpoint + '/tasks')).status).toBe(404);
  const approvalId = await fetch(endpoint + '/approvals').then(response => response.json()).then(items => items.find(item => item.action === 'plugin.invoke').id);
  await pending.getByRole('button', { name: '允许', exact: true }).click();
  await expect(page.getByText('插件写入已完成。', { exact: true })).toHaveCount(1);
  expect(writeCount()).toBe(initialWrites + 1);
  expect((await fetch(endpoint + '/approvals/' + approvalId + '/approve', { method: 'POST' })).status).toBe(409);

  await page.getByLabel('消息', { exact: true }).fill('创建 GitHub Issue，请等待我审核'); await page.getByLabel('消息', { exact: true }).press('Enter');
  await expect(pending).toBeVisible(); await pending.getByRole('button', { name: '拒绝', exact: true }).click();
  await expect(page.getByText('插件操作失败。', { exact: true })).toHaveCount(1); expect(writeCount()).toBe(initialWrites + 1);
  await page.getByLabel('消息', { exact: true }).fill('创建 GitHub Issue，检查权限变更'); await page.getByLabel('消息', { exact: true }).press('Enter');
  await expect(pending).toBeVisible();
  await fetch(endpoint + '/plugins/github/connection', { method: 'POST', body: JSON.stringify({ enabled: false, allow_writes: false }) });
  await fetch(endpoint + '/plugins/github/connection', { method: 'POST', body: JSON.stringify({ enabled: true, allow_writes: true }) });
  await pending.getByRole('button', { name: '允许', exact: true }).click();
  await expect(page.getByText('插件操作失败。', { exact: true })).toHaveCount(2); expect(writeCount()).toBe(initialWrites + 1);
  await fetch(endpoint + '/plugins/github/connection', { method: 'POST', body: JSON.stringify({ enabled: true, allow_writes: true }) });
  await page.getByLabel('审批模式', { exact: true }).selectOption('all');
  await page.getByLabel('消息', { exact: true }).fill('创建 GitHub Issue，使用当前审批模式'); await page.getByLabel('消息', { exact: true }).press('Enter');
  await expect(page.getByText('插件写入已完成。', { exact: true })).toHaveCount(2); expect(writeCount()).toBe(initialWrites + 2);
  await expect(pending).toHaveCount(0);
  for (const request of services.requests.filter(request => request.path === '/model/responses')) {
    for (const id of ['slack', 'github', 'notion', 'figma']) expect(JSON.stringify(request.body)).not.toContain('fixture-' + id + '-token');
  }
  await fetch(endpoint + '/plugins/github/connection', { method: 'DELETE' });
  expect(await readFile(join(directory, 'plugins/connections/github.json')).catch(error => error.code)).toBe('ENOENT');
  expect((await fetch(endpoint + '/plugins/github/invoke', { method: 'POST', body: JSON.stringify({ action: 'list_repositories', input: {} }) })).status).toBe(409);
});
test.afterAll(async () => { await stopCore(); await services?.close(); await rm(directory, { recursive: true, force: true }); });

test('Slack keeps bot access and supports personal channel, DM, and thread messaging without inviting a bot', async ({ page }, testInfo) => {
  test.setTimeout(60000);
  const call = (operation: string, body?: unknown) => fetch(endpoint + '/plugins/slack/' + operation, { method: 'POST', headers: { 'content-type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
  expect((await call('install')).ok).toBe(true);
  expect((await call('connection', { token: 'fixture-slack-token', enabled: true, allow_writes: false })).ok).toBe(true);
  const botDenied = await call('invoke', { action: 'channel_history', input: { channel: 'CUNJOINED' } });
  expect(botDenied.status).toBe(502); expect(await botDenied.text()).toContain('not_in_channel');
  await page.addInitScript(url => { localStorage.setItem('omnistream.url', url); localStorage.setItem('omnistream.locale', 'zh-CN'); }, endpoint);
  await page.goto('/chat/slack-user');
  await page.getByRole('button', { name: '插件市场', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '插件市场' });
  await dialog.getByRole('article', { name: 'Slack', exact: true }).getByRole('button', { name: '配置与使用' }).click();
  await expect(dialog.getByLabel('Slack 连接方式')).toHaveValue('token');
  await expect(dialog.getByText('当前账号：team-bot')).toBeVisible();
  await dialog.getByLabel('Slack 连接方式').selectOption('slack_user');
  await dialog.getByText('手动配置说明', { exact: true }).click();
  await expect(dialog.getByText(/无需邀请机器人到公共频道/)).toBeVisible();
  await expect(dialog.getByRole('button', { name: '验证并保存' })).toBeDisabled();
  await expect(dialog.getByRole('button', { name: '列出会话', exact: true })).toBeDisabled();
  for (const token of ['xoxb-wrong-token', 'xoxp-bot-identity-token', 'xoxp-bad-token']) {
    await dialog.getByLabel('访问令牌', { exact: true }).fill(token);
    await dialog.getByRole('button', { name: '验证并保存' }).click();
    await expect(dialog.getByRole('alert')).toBeVisible();
    const saved = JSON.parse(await readFile(join(directory, 'plugins/connections/slack.json'), 'utf8'));
    expect(saved.token).toBe('fixture-slack-token'); expect(saved.auth_mode).toBe('token');
  }
  await dialog.getByLabel('访问令牌', { exact: true }).fill('xoxp-fixture-user-token');
  await dialog.getByRole('button', { name: '验证并保存' }).click();
  await expect(dialog.getByText('当前账号：fixture-user')).toBeVisible();
  await expect(dialog.getByLabel('访问令牌', { exact: true })).toHaveValue('');
  await expect(dialog.getByRole('option', { name: '开启私信或群聊 · 写入' })).toHaveAttribute('disabled');
  expect((await call('invoke', { action: 'open_conversation', input: { users: 'U456' } })).status).toBe(409);
  expect(services.requests.filter(request => request.path === '/slack/conversations.open')).toHaveLength(0);
  await dialog.getByRole('button', { name: '列出会话', exact: true }).click();
  await expect(dialog.getByText('general', { exact: true })).toBeVisible();
  await dialog.getByLabel('选择操作').selectOption('channel_history');
  await dialog.getByLabel(/会话 ID/).fill('CUNJOINED');
  await dialog.getByRole('button', { name: '读取会话消息', exact: true }).click();
  await expect(dialog.getByText('Team update', { exact: true })).toBeVisible();
  for (const [type, text] of [['private_channel', 'private-team'], ['mpim', 'group-dm'], ['im', 'D123']]) {
    await dialog.getByLabel('选择操作').selectOption('list_channels');
    await dialog.getByLabel('会话类型').selectOption(type);
    await dialog.getByRole('button', { name: '列出会话', exact: true }).click();
    await expect(dialog.getByText(text, { exact: true })).toBeVisible();
    if (type === 'im') {
      await dialog.getByRole('button', { name: '下一页' }).click();
      await expect(dialog.getByRole('button', { name: '下一页' })).toHaveCount(0);
    }
  }
  await dialog.getByLabel('选择操作').selectOption('channel_history');
  await dialog.getByLabel(/会话 ID/).fill('D123');
  await dialog.getByLabel('起始时间戳（不含）').fill('123.000001');
  await dialog.getByRole('button', { name: '读取会话消息', exact: true }).click();
  await expect(dialog.getByText('Personal DM update', { exact: true })).toBeVisible();
  await dialog.getByLabel('选择操作').selectOption('thread_replies');
  await dialog.getByLabel(/会话 ID/).fill('CUNJOINED');
  await dialog.getByLabel(/线程主消息时间戳/).fill('123.456');
  await dialog.getByRole('button', { name: '读取线程回复', exact: true }).click();
  await expect(dialog.getByText('Thread reply', { exact: true })).toBeVisible();
  await dialog.getByRole('button', { name: '下一页' }).click();
  await expect(dialog.getByRole('button', { name: '下一页' })).toHaveCount(0);
  await dialog.getByLabel('允许写入此应用').check();
  await dialog.getByRole('button', { name: '验证并保存' }).click();
  await expect(dialog.getByText('连接配置已保存')).toBeVisible();
  await expect(dialog.getByText('Thread reply', { exact: true })).toHaveCount(0);
  for (const [users, conversation] of [['U456', 'D123'], ['U456,U789', 'G123']]) {
    await dialog.getByLabel('选择操作').selectOption('open_conversation');
    await dialog.getByLabel(/对方用户 ID/).fill(users);
    await dialog.getByRole('button', { name: '开启私信或群聊', exact: true }).click();
    await expect(dialog.getByText(conversation, { exact: true })).toBeVisible();
    await dialog.getByLabel('选择操作').selectOption('send_message');
    await dialog.getByLabel(/会话 ID/).fill(conversation);
    await dialog.getByLabel(/消息内容/).fill('Personal message to ' + conversation);
    await dialog.getByRole('button', { name: '发送消息', exact: true }).click();
    await expect(dialog.getByText('Personal message to ' + conversation, { exact: true })).toBeVisible();
  }
  expect(services.requests.find(request => request.path === '/slack/conversations.history' && request.query.get('oldest') === '123.000001')?.query.get('channel')).toBe('D123');
  expect(services.requests.find(request => request.path === '/slack/conversations.replies' && request.query.has('cursor'))?.query.get('ts')).toBe('123.456');
  for (const request of services.requests.filter(request => request.path === '/slack/chat.postMessage')) expect(request.headers.authorization).toBe('Bearer xoxp-fixture-user-token');
  const publicData = await fetch(endpoint + '/plugins/marketplace').then(response => response.text());
  expect(publicData).not.toContain('xoxp-fixture-user-token');
  expect(publicData).toContain('slack_user');
  expect(await page.evaluate(() => JSON.stringify(localStorage))).not.toContain('xoxp-fixture-user-token');
  await stopCore(); await startCore(); await page.reload();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole('button', { name: '切换侧栏' }).click();
  await page.getByRole('button', { name: '插件市场', exact: true }).click();
  await dialog.getByRole('article', { name: 'Slack', exact: true }).getByRole('button', { name: '配置与使用' }).click();
  await expect(dialog.getByLabel('Slack 连接方式')).toHaveValue('slack_user');
  await expect(dialog.getByText('当前账号：fixture-user')).toBeVisible();
  await dialog.getByRole('button', { name: '测试连接' }).click();
  await expect(dialog.getByText('连接验证成功')).toBeVisible();
  expect(await dialog.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  await dialog.screenshot({ path: testInfo.outputPath('slack-personal-mobile.png') });
  await dialog.getByLabel('Slack 连接方式').selectOption('token');
  await dialog.getByLabel('访问令牌', { exact: true }).fill('fixture-slack-token');
  await dialog.getByRole('button', { name: '验证并保存' }).click();
  await expect(dialog.getByText('当前账号：team-bot')).toBeVisible();
  await expect(dialog.getByLabel('允许写入此应用')).not.toBeChecked();
  expect((await call('connection', { token: 'xoxp-fixture-user-token', auth_mode: 'slack_user', enabled: true, allow_writes: true })).ok).toBe(true);
  await page.keyboard.press('Escape');
  await page.getByLabel('消息', { exact: true }).fill('读取 Slack 私信');
  await page.getByLabel('消息', { exact: true }).press('Enter');
  await expect(page.locator('.chat-message.assistant').getByText('插件读取已完成。', { exact: true })).toBeVisible();
  const writeCount = () => services.requests.filter(request => request.path === '/slack/chat.postMessage').length;
  const beforeWrites = writeCount();
  await page.getByLabel('消息', { exact: true }).fill('发送 Slack 私信，请等待审批');
  await page.getByLabel('消息', { exact: true }).press('Enter');
  const approval = page.locator('.pending-request');
  await expect(approval).toContainText('fixture-user');
  await expect(approval).toContainText('Assistant personal message');
  expect(writeCount()).toBe(beforeWrites);
  expect((await call('connection', { token: 'fixture-slack-token', auth_mode: 'token', enabled: true, allow_writes: true })).ok).toBe(true);
  await approval.getByRole('button', { name: '允许', exact: true }).click();
  await expect(page.getByText('插件操作失败。', { exact: true })).toHaveCount(1);
  expect(writeCount()).toBe(beforeWrites);
  expect((await call('connection', { token: 'xoxp-fixture-user-token', auth_mode: 'slack_user', enabled: true, allow_writes: true })).ok).toBe(true);
  await page.getByLabel('消息', { exact: true }).fill('发送 Slack 私信，使用个人账号');
  await page.getByLabel('消息', { exact: true }).press('Enter');
  await expect(approval).toContainText('fixture-user');
  await approval.getByRole('button', { name: '允许', exact: true }).click();
  await expect(page.getByText('插件写入已完成。', { exact: true })).toHaveCount(1);
  expect(writeCount()).toBe(beforeWrites + 1);
  const message = services.requests.filter(request => request.path === '/slack/chat.postMessage').at(-1);
  expect(message?.headers.authorization).toBe('Bearer xoxp-fixture-user-token');
  expect(message?.body.text).toBe('Assistant personal message');
  for (const request of services.requests.filter(request => request.path === '/model/responses')) expect(JSON.stringify(request.body)).not.toContain('xoxp-fixture-user-token');
});

test('Slack listens continuously with selectable scope, durable sync, account isolation, and personal replies', async ({ page }, testInfo) => {
  test.setTimeout(120000);
  const call = (path: string, input?: unknown) => fetch(endpoint + '/plugins/slack/' + path, { method: input === undefined ? 'GET' : 'POST', headers: { 'content-type': 'application/json' }, body: input === undefined ? undefined : JSON.stringify(input) });
  const state = () => call('inbox').then(response => response.json());
  const read = (channel: string) => call('inbox/read', { channel }).then(response => response.json());
  const emit = async (event: Record<string, unknown>, options?: { team?: string; owner?: string; eventId?: string }) => {
    const envelope = services.slackEvent(event, { owner: 'USTREAM', ...options });
    await expect.poll(() => services.acknowledgements.includes(envelope)).toBe(true);
  };
  expect((await call('install', {})).ok).toBe(true);
  await page.addInitScript(url => { localStorage.setItem('omnistream.url', url); localStorage.setItem('omnistream.locale', 'zh-CN'); }, endpoint);
  await page.goto('/chat/slack-stream');
  await page.getByRole('button', { name: '插件市场', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '插件市场' });
  const manage = () => dialog.getByRole('article', { name: 'Slack', exact: true }).getByRole('button', { name: '配置与使用' }).click();
  await manage();
  await dialog.getByLabel('Slack 连接方式').selectOption('slack_user');
  await dialog.getByLabel('访问令牌', { exact: true }).fill('xoxp-stream-user-token');
  await dialog.getByLabel('持续接收新消息').check();
  await expect(dialog.getByLabel('监听范围')).toHaveValue('related');
  await expect(dialog.getByRole('button', { name: '验证并保存' })).toBeDisabled();
  await dialog.getByLabel('App-level Token', { exact: true }).fill('xapp-fixture-private-token');
  await dialog.getByRole('button', { name: '验证并保存' }).click();
  await expect(dialog.getByText('连接配置已保存')).toBeVisible();
  await expect(dialog.getByLabel('App-level Token', { exact: true })).toHaveValue('');
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Slack 收件箱', exact: true }).click();
  await expect(page.getByText('正在接收新消息', { exact: true })).toBeVisible();
  await expect(page.getByText('还没有同步消息', { exact: true })).toBeVisible();
  await emit({ type: 'message', channel: 'C123', channel_type: 'channel', user: 'U456', text: 'Unrelated channel traffic', ts: '1700000000.000001' });
  await emit({ type: 'message', channel: 'D123', channel_type: 'im', user: 'U456', text: 'Live personal message', ts: '1700000001.000001' }, { eventId: 'durable-dm' });
  await emit({ type: 'message', channel: 'D123', channel_type: 'im', user: 'U456', text: 'Live personal message', ts: '1700000001.000001' }, { eventId: 'durable-dm' });
  expect((await read('D123')).messages).toHaveLength(1);
  await expect(page.locator('.slack-message p').getByText('Live personal message', { exact: true })).toBeVisible();
  await expect(page.getByLabel('Slack 消息内容', { exact: true })).toBeDisabled();
  await emit({ type: 'message', channel: 'DOTHER', channel_type: 'im', user: 'U456', text: 'Other account secret', ts: '1700000002.000001' }, { owner: 'UOTHER' });
  await emit({ type: 'message', channel: 'DTEAM', channel_type: 'im', user: 'U456', text: 'Other workspace secret', ts: '1700000002.000001' }, { team: 'TOTHER' });
  await emit({ type: 'message', channel: 'G123', channel_type: 'mpim', user: 'U456', text: 'Live group DM', ts: '1700000003.000001' });
  await emit({ type: 'message', channel: 'C123', channel_type: 'channel', user: 'U456', text: 'Hi <@USTREAM>, please review', ts: '1700000004.000001' });
  await emit({ type: 'message', channel: 'C123', channel_type: 'channel', user: 'U789', text: 'Related thread response', ts: '1700000005.000001', thread_ts: '1700000004.000001' });
  const snapshot = await state();
  expect(snapshot.conversations.map((item: { channel: string }) => item.channel).sort()).toEqual(['C123', 'D123', 'G123']);
  expect((await read('C123')).messages.map((item: { text: string }) => item.text)).toEqual(['Hi <@USTREAM>, please review', 'Related thread response']);
  expect(JSON.stringify(snapshot)).not.toContain('xapp-fixture-private-token');
  expect(JSON.stringify(snapshot)).not.toContain('xoxp-stream-user-token');
  await emit({ type: 'message', subtype: 'message_changed', channel: 'D123', channel_type: 'im', event_ts: '1700000006.000001', message: { ts: '1700000001.000001', user: 'U456', text: 'Edited personal message' } });
  await expect(page.locator('.slack-message p').getByText('Edited personal message', { exact: true })).toBeVisible();
  expect((await call('connection', { enabled: true, allow_writes: true })).ok).toBe(true);
  await expect(page.getByLabel('Slack 消息内容', { exact: true })).toBeEnabled();
  await page.getByLabel('Slack 消息内容', { exact: true }).fill('Personal reply from inbox');
  await page.getByRole('button', { name: '发送到 Slack', exact: true }).click();
  await expect(page.getByText('消息已发送到 Slack', { exact: true })).toBeVisible();
  const sent = (await read('D123')).messages.find((message: { text: string }) => message.text === 'Personal reply from inbox');
  expect(sent.own).toBe(true);
  await expect.poll(() => services.activeSockets).toBe(1);
  await emit({ type: 'message', channel: 'D123', channel_type: 'im', user: 'USTREAM', text: 'Personal reply from inbox', ts: sent.ts });
  expect((await read('D123')).messages.filter((message: { ts: string }) => message.ts === sent.ts)).toHaveLength(1);
  await page.getByRole('button', { name: /频道 · C123/ }).click();
  const mention = page.locator('.slack-message').filter({ has: page.getByText('Hi <@USTREAM>, please review', { exact: true }) });
  await mention.getByRole('button', { name: '回复线程', exact: true }).click();
  await page.getByLabel('Slack 消息内容', { exact: true }).fill('Personal thread reply');
  await page.getByRole('button', { name: '发送到 Slack', exact: true }).click();
  await expect(page.getByText('消息已发送到 Slack', { exact: true })).toBeVisible();
  const lastSend = services.requests.filter(request => request.path === '/slack/chat.postMessage').at(-1);
  expect(lastSend?.body.thread_ts).toBe('1700000004.000001');
  expect(lastSend?.headers.authorization).toBe('Bearer xoxp-stream-user-token');
  await page.getByRole('button', { name: '配置与使用', exact: true }).click();
  await manage();
  await dialog.getByLabel('监听范围').selectOption('all');
  await dialog.getByRole('button', { name: '验证并保存' }).click();
  await expect(dialog.getByText('连接配置已保存')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect.poll(async () => (await state()).listener.state).toBe('listening');
  await emit({ type: 'message', channel: 'C456', channel_type: 'channel', user: 'U456', text: 'All-scope channel message', ts: '1700000010.000001' });
  expect((await read('C456')).messages).toHaveLength(1);
  const connections = services.socketConnections;
  services.disconnectSockets();
  await expect.poll(() => services.socketConnections).toBeGreaterThan(connections);
  await expect.poll(async () => (await state()).listener.state).toBe('listening');
  await stopCore(); await startCore(); await page.reload();
  await expect(page.getByText('正在接收新消息', { exact: true })).toBeVisible();
  await expect.poll(() => services.activeSockets).toBe(1);
  expect((await read('D123')).messages).toHaveLength(2);
  await page.getByRole('button', { name: /私信 · D123/ }).click();
  await emit({ type: 'message', subtype: 'message_deleted', channel: 'D123', channel_type: 'im', event_ts: '1700000020.000001', deleted_ts: '1700000001.000001', previous_message: { ts: '1700000001.000001', user: 'U456', text: 'Edited personal message' } });
  await expect(page.locator('.slack-message p').getByText('消息已删除', { exact: true })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('slack-inbox-desktop.png') });
  await page.setViewportSize({ width: 390, height: 844 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('slack-inbox-mobile.png') });
  expect((await call('connection', { enabled: true, allow_writes: true, listen: false })).ok).toBe(true);
  await expect.poll(() => services.activeSockets).toBe(0);
  await expect(page.getByText('监听已停止', { exact: true })).toBeVisible();
  expect((await call('connection', { token: 'xoxp-other-user-token', enabled: true, allow_writes: true, listen: false })).ok).toBe(true);
  await expect(page.getByText('当前账号：other-user', { exact: true })).toBeVisible();
  await expect(page.getByText('还没有同步消息', { exact: true })).toBeVisible();
  expect((await call('inbox/send', { team_id: 'T123', user_id: 'USTREAM', channel: 'D123', text: 'Stale account send' })).status).toBe(409);
  expect(services.requests.some(request => request.path === '/slack/chat.postMessage' && request.body.text === 'Stale account send')).toBe(false);
  expect((await call('connection', { token: 'xoxp-stream-user-token', enabled: true, allow_writes: false, listen: false })).ok).toBe(true);
  expect((await read('D123')).messages).toHaveLength(2);
});

test('keeps the market responsive and discards stale connection checks during concurrent changes', async ({ page }) => {
  test.setTimeout(60000);
  const call = (id: string, operation: string, input?: unknown, method = 'POST') => fetch(endpoint + '/plugins/' + id + operation, { method, headers: { 'content-type': 'application/json' }, body: input === undefined ? undefined : JSON.stringify(input), signal: AbortSignal.timeout(10000) });
  for (const id of ['github', 'slack']) expect((await call(id, '/install')).ok).toBe(true);
  for (const removeManifest of [false, true]) {
    const gate = services.hold('GET', '/github/user');
    const pending = call('github', '/connection', { token: 'fixture-github-token', enabled: true, allow_writes: false });
    try {
      await gate.started;
      expect((await fetch(endpoint + '/plugins/marketplace', { signal: AbortSignal.timeout(1500) })).ok).toBe(true);
      if (!removeManifest) {
        expect((await call('slack', '/connection', { token: 'fixture-slack-token', enabled: true, allow_writes: false })).ok).toBe(true);
        expect((await call('github', '/connection', undefined, 'DELETE')).ok).toBe(true);
      } else {
        expect((await call('github', '', undefined, 'DELETE')).ok).toBe(true);
        expect((await call('github', '/install')).ok).toBe(true);
      }
      gate.release();
      expect((await pending).status).toBe(409);
      expect(await readFile(join(directory, 'plugins/connections/github.json')).catch(error => error.code)).toBe('ENOENT');
    } finally { gate.release(); await pending.catch(() => undefined); }
  }
  expect((await call('github', '/connection', { token: 'fixture-github-token', enabled: true, allow_writes: false })).ok).toBe(true);
  const verifying = services.hold('GET', '/github/user');
  const pendingTest = call('github', '/test');
  try {
    await verifying.started;
    expect((await call('github', '/connection', { enabled: false, allow_writes: false })).ok).toBe(true);
    verifying.release(); expect((await pendingTest).status).toBe(409);
  } finally { verifying.release(); await pendingTest.catch(() => undefined); }
  expect((await call('github', '/connection', { enabled: true, allow_writes: false })).ok).toBe(true);

  await page.addInitScript(url => { localStorage.setItem('omnistream.url', url); localStorage.setItem('omnistream.locale', 'zh-CN'); }, endpoint);
  await page.goto('/chat/plugin-concurrency');
  await page.getByRole('button', { name: '插件市场', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '插件市场' });
  await dialog.getByRole('article', { name: 'GitHub', exact: true }).getByRole('button', { name: '配置与使用' }).click();
  const reading = services.hold('GET', '/github/user/repos');
  const pendingRead = page.waitForResponse(response => response.url().endsWith('/plugins/github/invoke') && response.status() === 200);
  try {
    await dialog.getByRole('button', { name: '列出仓库', exact: true }).click(); await reading.started;
    await dialog.getByRole('button', { name: '返回插件市场' }).click();
    await dialog.getByRole('article', { name: 'Slack', exact: true }).getByRole('button', { name: '配置与使用' }).click();
    await dialog.getByRole('button', { name: '测试连接' }).click();
    await expect(dialog.getByText('连接验证成功')).toBeVisible({ timeout: 1500 });
    expect((await call('github', '/connection', { enabled: false, allow_writes: false })).ok).toBe(true);
    await dialog.getByRole('button', { name: '返回插件市场' }).click();
    await dialog.getByRole('article', { name: 'GitHub', exact: true }).getByRole('button', { name: '配置与使用' }).click();
    await dialog.getByRole('button', { name: '列出仓库', exact: true }).click();
    await expect(dialog.getByLabel('启用此插件')).not.toBeChecked();
    await expect(dialog.getByText(/连接设置已更新/)).toBeVisible();
    expect(services.requests.filter(request => request.path === '/github/user/repos')).toHaveLength(1);
    reading.release(); expect((await pendingRead).ok()).toBe(true);
    const stored = JSON.parse(await readFile(join(directory, 'plugins/connections/github.json'), 'utf8'));
    expect(stored.enabled).toBe(false);
  } finally { reading.release(); await pendingRead.catch(() => undefined); }
});

test('download selected plugins, share Core state, restart, and uninstall on mobile', async ({ page, browser }, testInfo) => {
  await page.addInitScript(url => { localStorage.setItem('omnistream.url', url); localStorage.setItem('omnistream.locale', 'zh-CN'); }, endpoint);
  await page.goto('/chat/plugins-inbox');
  await page.getByRole('button', { name: '插件市场', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '插件市场' });
  await expect(dialog.getByRole('article')).toHaveCount(5);
  expect((await dialog.boundingBox())!.width).toBeGreaterThan(700);
  await expect(dialog.getByRole('button', { name: '下载', exact: true })).toHaveCount(5);
  await expect(dialog.getByRole('button', { name: '已安装 0' })).toBeVisible();
  await expect(dialog.getByText(/下载后连接账号/)).toBeVisible();
  await dialog.getByRole('textbox').fill('github');
  await expect(dialog.getByRole('article')).toHaveCount(1);
  await dialog.getByRole('article', { name: 'GitHub' }).getByRole('button', { name: '下载', exact: true }).click();
  await expect(dialog.getByRole('button', { name: '卸载', exact: true })).toBeVisible();
  await dialog.getByRole('textbox').fill('');
  for (const name of ['Slack', 'Notion', 'Figma']) {
    const card = dialog.getByRole('article', { name, exact: true });
    await card.getByRole('button', { name: '下载', exact: true }).click();
    await expect(card.getByRole('button', { name: '卸载', exact: true })).toBeVisible();
  }
  const manifest = JSON.parse(await readFile(join(directory, 'plugins/github.json'), 'utf8'));
  expect(manifest.id).toBe('github');
  expect((await readdir(join(directory, 'plugins'))).filter(name => name.endsWith('.json')).sort()).toEqual(['figma.json', 'github.json', 'notion.json', 'slack.json']);
  await dialog.screenshot({ path: testInfo.outputPath('marketplace-desktop.png') });

  const other = await browser.newContext();
  try {
    await other.addInitScript(url => { localStorage.setItem('omnistream.url', url); localStorage.setItem('omnistream.locale', 'en-US'); }, endpoint);
    const second = await other.newPage();
    await second.goto('http://127.0.0.1:1436/chat/another-browser');
    await second.getByRole('button', { name: 'Plugin marketplace', exact: true }).click();
    await expect(second.getByRole('dialog').getByRole('button', { name: 'Uninstall', exact: true })).toHaveCount(4);
  } finally { await other.close(); }

  await stopCore();
  await startCore();
  await page.reload();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole('button', { name: '切换侧栏' }).click();
  await page.getByRole('button', { name: '插件市场', exact: true }).click();
  await expect(dialog.getByRole('button', { name: '已安装 4' })).toBeVisible();
  await dialog.getByRole('button', { name: '已安装 4' }).click();
  const github = dialog.getByRole('article', { name: 'GitHub', exact: true });
  await github.getByRole('button', { name: '卸载', exact: true }).click();
  await expect(github).toHaveCount(0);
  await expect(dialog.getByRole('article')).toHaveCount(3);
  expect((await readdir(join(directory, 'plugins'))).filter(name => name.endsWith('.json')).sort()).toEqual(['figma.json', 'notion.json', 'slack.json']);
  expect(await dialog.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await dialog.screenshot({ path: testInfo.outputPath('marketplace-mobile.png') });
  await page.keyboard.press('Escape');
  await expect(dialog).not.toBeVisible();
  await page.getByRole('button', { name: '切换侧栏' }).click();
  await page.getByRole('button', { name: '插件市场', exact: true }).click();
  await dialog.getByRole('article', { name: 'GitHub' }).getByRole('button', { name: '下载', exact: true }).click();
  await expect(dialog.getByRole('button', { name: '已安装 4' })).toBeVisible();
});
