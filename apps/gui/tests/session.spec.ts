import { test, expect } from '@playwright/test';
import { createServer, Server } from 'node:http';
import { mkdtemp, mkdir, writeFile, rm, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { spawn, ChildProcess } from 'node:child_process';

let core: ChildProcess;
let model: Server;
let directory: string;
let requests: Record<string, unknown>[] = [];
let catalogRequests = 0;
const endpoint = 'http://127.0.0.1:4331';
const picture = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aE2kAAAAASUVORK5CYII=';

test.beforeAll(async () => {
  directory = await mkdtemp(join(tmpdir(), 'conversation-e2e-'));
  model = createServer((req, res) => {
    if (req.method === 'GET' && req.url?.endsWith('/models')) {
      catalogRequests++;
      const other = req.url.startsWith('/other');
      const models = other ? [{ id: 'other-model' }] : [{ id: 'test-model' }, { id: 'plain-model' }];
      setTimeout(() => { res.setHeader('content-type', 'application/json'); res.end(JSON.stringify({ data: models, reasoning_efforts: other ? ['low', 'medium', 'high'] : ['high', 'max'] })); }, 350);
      return;
    }
    let body = '';
    req.on('data', chunk => { body += chunk; });
    req.on('end', () => {
      const value = JSON.parse(body); requests.push(value);
      const input = JSON.stringify(value.input ?? value.messages);
      if (input.includes('根据用户请求判断外部 Agent 结果是否需要总结')) {
        res.setHeader('content-type', 'application/json');
        res.end(JSON.stringify({ output_text: JSON.stringify({ summary: '检查结论：已完成，测试通过。' }) })); return;
      }
      const currentPrompt = input.slice(input.lastIndexOf('用户输入：'));
      const decision = currentPrompt.includes('委派验证') ? { kind: 'delegate', project: directory, reason: 'inspect' } : { kind: 'reply', message: currentPrompt.includes('图示验证') ? '```mermaid\nflowchart LR\nA[需求] --> B[实现]\n```' : '已收到消息。\n\n```javascript\nconst result = 42;\n```' };
      res.setHeader('content-type', 'application/json');
      res.end(JSON.stringify({ output_text: JSON.stringify(decision) }));
    });
  });
  await new Promise<void>(done => model.listen(4332, '127.0.0.1', done));
  await writeFile(join(directory, 'agent.cjs'), `
const readline = require('node:readline');
const send = value => console.log(JSON.stringify(value));
let policy, longOutput;
readline.createInterface({input:process.stdin}).on('line', line => {
 const r = JSON.parse(line);
 if (r.method === 'initialize') send({id:r.id,result:{}});
 if (r.method === 'thread/start' || r.method === 'thread/resume') { policy=r.params.approvalPolicy; require('node:fs').appendFileSync(${JSON.stringify(join(directory, 'agent-settings.jsonl'))}, JSON.stringify(r)+'\\n'); send({id:r.id,result:{thread:{id:r.params.threadId || 'fixture-thread'}}}); }
 if (r.method === 'turn/start') {
  const current=(r.params.input[0].text.split('原始请求：')[1] || '').split('已有信息：')[0];
  longOutput=current.includes('长输出');
  send({id:r.id,result:{turn:{id:'turn-1'}}});
  send({method:'item/agentMessage/delta',params:{threadId:'fixture-thread',turnId:'turn-1',delta:'正在查看项目'}});
  if (current.includes('表单验证')) send({id:'question',method:'mcpServer/elicitation/request',params:{threadId:'fixture-thread',turnId:'turn-1',mode:'form',message:'补充执行参数',requestedSchema:{type:'object',properties:{name:{type:'string',title:'名称'},count:{type:'integer',title:'数量',minimum:1},enabled:{type:'boolean',title:'启用'}},required:['name','count','enabled']}}});
  else send({id:'question',method:'item/tool/requestUserInput',params:{threadId:'fixture-thread',turnId:'turn-1',questions:[{id:'q',header:'范围',question:'请选择检查范围',options:[{label:'源码',description:'检查源码'}]}]}});
 }
 if(r.id==='question' && r.result) {
  send({method:'item/completed',params:{threadId:'fixture-thread',turnId:'turn-1',item:{type:'agentMessage',text:longOutput ? '检查结果。'.repeat(500) : '已完成源码检查'}}});
  send({method:'turn/completed',params:{threadId:'fixture-thread',turn:{id:'turn-1',status:'completed'}}});
 }
});`);
  await writeFile(join(directory, 'config.toml'), `
model_provider = "mock"
model = "test-model"
[model_providers.mock]
name = "测试 Provider"
base_url = "http://127.0.0.1:4332"
models = [{ id = "test-model" }, { id = "plain-model" }]
[model_providers.mock.auth]
command = "printf"
args = ["fixture-key"]
[model_providers.other]
name = "另一 Provider"
base_url = "http://127.0.0.1:4332/other"
models = [{ id = "other-model" }]
[model_providers.other.auth]
command = "printf"
args = ["fixture-key"]
[agents.default]
command = "node"
protocol = "app-server"
args = [${JSON.stringify(join(directory, 'agent.cjs'))}]
`);
  await mkdir(join(directory, 'skills', 'fixture'), { recursive: true });
  await writeFile(join(directory, 'skills', 'fixture', 'SKILL.md'), '自定义规则标记：fixture-skill-loaded');
  core = spawn(resolve('../../target/debug/omnistream'), ['serve', '127.0.0.1:4331'], { cwd: directory, env: { ...process.env, OMNISTREAM_HOME: directory }, stdio: 'pipe' });
  core.stderr?.on('data', data => process.stderr.write(data));
  await expect.poll(async () => fetch(`${endpoint}/health`).then(r => r.ok).catch(() => false)).toBe(true);
});
test.afterAll(async () => {
  if (core && core.exitCode === null) { core.kill(); await new Promise(done => core.once('exit', done)); }
  await new Promise<void>(done => model.close(() => done()));
  await rm(directory, { recursive: true, force: true });
});
test.beforeEach(async ({ page }) => {
  await page.addInitScript(({ url, project }) => { localStorage.setItem('omnistream.url', url); localStorage.setItem('omnistream.projects', JSON.stringify([project])); }, { url: endpoint, project: directory });
  await page.goto('/');
  await expect(page.getByLabel('消息', { exact: true })).toBeVisible();
});

test('legacy task pages return to chat without loading development resources', async ({ page }) => {
  const legacyRequests: string[] = [];
  page.on('request', request => {
    if (/\/tasks(?:\/|\?|$)|\/task-proposals(?:\/|$)/.test(request.url()) && request.resourceType() === 'fetch') legacyRequests.push(request.url());
  });
  await page.goto('/tasks/41');
  await expect(page).toHaveURL(/\/chat\/[^/]+$/);
  await expect(page.getByLabel('消息', { exact: true })).toBeVisible();
  await expect(page.getByText('历史开发任务', { exact: false })).toHaveCount(0);
  await page.getByRole('button', { name: '长期任务', exact: true }).click();
  await expect(page.getByRole('dialog', { name: '长期任务', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '关闭', exact: true }).click();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole('button', { name: '切换侧栏', exact: true }).click();
  await expect(page.getByRole('button', { name: '长期任务', exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  expect(legacyRequests).toEqual([]);
});

test('session preferences, editor keys, rename and provider/model linkage', async ({ page }) => {
  await page.getByLabel('审批模式', { exact: true }).selectOption('all');
  await expect(page.getByLabel('审批模式', { exact: true })).toHaveValue('all');
  await page.reload();
  await expect(page.getByLabel('审批模式', { exact: true })).toHaveValue('all');
  await page.getByRole('button', { name: '新建会话', exact: true }).click();
  await expect(page.getByLabel('审批模式', { exact: true })).toHaveValue('default');
  await page.getByRole('button', { name: '选择模型', exact: true }).click();
  await expect(page.getByRole('status', { name: '正在加载模型' })).toBeVisible();
  await expect(page.getByRole('button', { name: '思考等级 max', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '思考等级 low', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '思考等级 high', exact: true }).click();
  await expect(page.getByRole('button', { name: '选择模型', exact: true })).toContainText('high');
  await page.reload();
  await expect(page.getByRole('button', { name: '选择模型', exact: true })).toContainText('high');
  await page.getByRole('button', { name: '选择模型', exact: true }).click();
  await page.getByRole('button', { name: '模型 plain-model', exact: true }).click();
  await page.getByRole('button', { name: '思考等级 默认', exact: true }).click();
  await expect(page.getByRole('button', { name: '选择模型', exact: true })).toContainText('plain-model');
  await page.getByRole('button', { name: '选择模型', exact: true }).click();
  await page.getByRole('button', { name: 'Provider 另一 Provider', exact: true }).click();
  await page.getByRole('button', { name: '模型 other-model', exact: true }).click();
  await page.getByRole('button', { name: '思考等级 默认', exact: true }).click();
  await expect(page.getByRole('button', { name: '选择模型', exact: true })).toContainText('other-model');
  await page.getByRole('button', { name: '新建会话', exact: true }).click();
  await expect(page.getByRole('button', { name: '选择模型', exact: true })).toContainText('other-model');
  const editor = page.getByLabel('消息', { exact: true });
  await editor.fill('第一行'); await editor.press('Shift+Enter'); await editor.press('a');
  expect(await editor.innerText()).toContain('\n');
  await editor.press('Enter');
  await expect(page.locator('.chat-messages').getByText('已收到消息。', { exact: true })).toBeVisible();
  expect(requests.at(-1)?.model).toBe('other-model');
  expect(JSON.stringify(requests.at(-1))).toContain('fixture-skill-loaded');
  await editor.fill('/Rename'); await editor.press('Enter');
  await page.getByLabel('会话名称').fill('我的工作会话'); await page.getByRole('button', { name: '保存', exact: true }).click();
  await expect(page.getByRole('button', { name: '我的工作会话', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '复制代码', exact: true })).toBeVisible();
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.getByRole('button', { name: '复制代码', exact: true }).click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe('const result = 42;');
  await page.screenshot({ path: 'test-results/conversation-desktop.png', fullPage: true });
  core.kill(); await new Promise(done => core.once('exit', done));
  core = spawn(resolve('../../target/debug/omnistream'), ['serve', '127.0.0.1:4331'], { cwd: directory, env: { ...process.env, OMNISTREAM_HOME: directory }, stdio: 'pipe' });
  await expect.poll(async () => fetch(`${endpoint}/health`).then(r => r.ok).catch(() => false)).toBe(true);
  await page.reload();
  await expect(page.getByRole('button', { name: '我的工作会话', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '选择模型', exact: true })).toContainText('other-model');
});

test('attachments preview, project token and mobile layout', async ({ page }) => {
  await page.locator('input[type=file]').setInputFiles([
    { name: 'one.png', mimeType: 'image/png', buffer: Buffer.from(picture, 'base64') },
    { name: 'two.png', mimeType: 'image/png', buffer: Buffer.from(picture, 'base64') },
    { name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('用户附件') },
  ]);
  await page.getByRole('button', { name: 'one.png', exact: true }).click();
  await expect(page.getByRole('dialog', { name: '图片预览' })).toBeVisible();
  const image = page.locator('.image-stage img');
  await expect.poll(() => image.evaluate(element => (element as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);
  await page.getByRole('button', { name: '放大', exact: true }).click();
  await expect(image).toHaveAttribute('style', /scale\(1.25\)/);
  await image.press('ArrowRight');
  await expect(image).toHaveAttribute('style', /translate\(30px/);
  await page.getByRole('button', { name: '还原大小', exact: true }).click();
  await expect(image).toHaveAttribute('style', /scale\(1\)/);
  await expect(page.getByRole('button', { name: '上一张', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '下一张', exact: true }).click();
  await expect(page.getByRole('button', { name: '下一张', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '关闭预览' }).click();
  await expect(page.getByRole('button', { name: 'notes.txt', exact: true })).toBeDisabled();
  const editor = page.getByLabel('消息', { exact: true });
  await editor.fill('@conversation');
  await page.getByRole('option', { name: directory, exact: true }).click();
  await expect(page.locator('.editor-reference')).toHaveCount(1);
  await editor.press('End'); await editor.press('Space'); await editor.press('Enter');
  await expect(page.locator('.chat-messages').getByText('已收到消息。', { exact: true })).toBeVisible();
  expect(JSON.stringify(requests.at(-1))).toContain('input_image');
  expect(JSON.stringify(requests.at(-1))).toContain('用户附件');
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: 'test-results/conversation-mobile.png', fullPage: true });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('external Agent timeline and structured input remains interactive in all mode', async ({ page }) => {
  await page.getByLabel('审批模式', { exact: true }).selectOption('all');
  await expect(page.getByLabel('审批模式', { exact: true })).toHaveValue('all');
  await page.getByLabel('消息', { exact: true }).fill('委派验证');
  await page.getByLabel('消息', { exact: true }).press('Enter');
  await expect(page.getByText('等待用户', { exact: true })).toBeVisible();
  await page.getByLabel('请选择检查范围').selectOption('源码');
  await page.getByRole('button', { name: '提交', exact: true }).click();
  await expect(page.getByText('已完成源码检查', { exact: true })).toBeVisible();
  // Normal completion keeps the output without a status badge.
  await expect(page.locator('.agent-call .status')).toHaveCount(0);
  await page.reload();
  await expect(page.getByText('已完成源码检查', { exact: true })).toBeVisible();
  await page.getByLabel('消息', { exact: true }).fill('继续委派验证');
  await page.getByLabel('消息', { exact: true }).press('Enter');
  await expect(page.getByText('等待用户', { exact: true })).toBeVisible();
  await expect(page.locator('.pending-request')).toHaveCount(1);
  await expect(page.getByText('正在查看项目', { exact: true })).toHaveCount(1);
  await page.getByRole('button', { name: '取消执行', exact: true }).click();
  await expect(page.getByText('已取消', { exact: true }).last()).toBeVisible();
  await expect(page.locator('.pending-request')).toHaveCount(0);
  const runs = (await readFile(join(directory, 'agent-settings.jsonl'), 'utf8')).trim().split('\n').map(line => JSON.parse(line));
  expect(runs.at(-1).method).toBe('thread/resume');
  expect(runs.at(-1).params.approvalPolicy).toBe('never');
});

test('pasted attachment is an atomic inline reference and can be removed', async ({ page }) => {
  const editor = page.getByLabel('消息', { exact: true });
  await editor.fill('之前 ');
  await editor.press('End');
  await editor.evaluate((element, base64) => {
    const data = new DataTransfer();
    data.items.add(new File([Uint8Array.from(atob(base64), char => char.charCodeAt(0))], 'pasted.png', { type: 'image/png' }));
    element.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
  }, picture);
  await expect(page.locator('.editor-reference')).toHaveText('[Image1]');
  await page.locator('.editor-reference').click();
  await expect(page.getByRole('dialog', { name: '图片预览' })).toBeVisible();
  await page.getByRole('button', { name: '关闭预览' }).click();
  await editor.click(); await editor.press('Control+End'); await editor.press('Backspace'); await editor.press('Backspace');
  await expect(page.locator('.editor-reference')).toHaveCount(0);
  await editor.press('Enter');
  await expect(page.locator('.chat-messages').getByText('已收到消息。', { exact: true })).toBeVisible();
  expect(JSON.stringify(requests.at(-1))).not.toContain('input_image');
});

test('diagrams render and long external results are summarized when needed', async ({ page }) => {
  const editor = page.getByLabel('消息', { exact: true });
  await editor.fill('图示验证'); await editor.press('Enter');
  await expect(page.locator('.diagram svg')).toBeVisible();
  await editor.fill('长输出委派验证'); await editor.press('Enter');
  await page.getByLabel('请选择检查范围').selectOption('源码');
  await page.getByRole('button', { name: '提交', exact: true }).click();
  await expect(page.getByText('检查结论：已完成，测试通过。', { exact: true })).toBeVisible();
});

test('approval stays with its source conversation and settings are scoped', async ({ page }) => {
  const original = await page.evaluate(() => localStorage.getItem('omnistream.conversation'));
  await page.getByLabel('审批模式', { exact: true }).selectOption('safe');
  await expect(page.getByLabel('审批模式', { exact: true })).toHaveValue('safe');
  await page.getByLabel('消息', { exact: true }).fill('隔离委派验证');
  await page.getByLabel('消息', { exact: true }).press('Enter');
  await expect(page.locator('.pending-request')).toHaveCount(1);
  await page.getByRole('button', { name: '新建会话', exact: true }).click();
  await expect(page.locator('.pending-request')).toHaveCount(0);
  await expect(page.getByLabel('审批模式', { exact: true })).toHaveValue('default');
  await page.evaluate(id => localStorage.setItem('omnistream.conversation', id!), original);
  await page.reload();
  await expect(page.getByLabel('审批模式', { exact: true })).toHaveValue('safe');
  await expect(page.locator('.pending-request')).toHaveCount(1);
  await page.getByRole('button', { name: '取消执行', exact: true }).click();
  await expect(page.locator('.pending-request')).toHaveCount(0);
});

test('MCP structured form submits typed values', async ({ page }) => {
  await page.getByLabel('消息', { exact: true }).fill('表单验证，委派验证');
  await page.getByLabel('消息', { exact: true }).press('Enter');
  await expect(page.getByText('补充执行参数', { exact: true })).toBeVisible();
  await page.getByLabel('名称', { exact: true }).fill('示例');
  await page.getByLabel('数量', { exact: true }).fill('2');
  await page.getByLabel('启用', { exact: true }).check();
  await page.getByRole('button', { name: '提交', exact: true }).click();
  await expect(page.getByText('已完成源码检查', { exact: true })).toBeVisible();
});

test('failed conversation requests stop polling until manual retry', async ({ page }) => {
  let count = 0;
  let fail = true;
  await page.route('**/conversation/detail', async route => {
    count++;
    if (fail) await route.fulfill({ status: 404, contentType: 'application/json', body: '{"error":"not found"}' });
    else await route.continue();
  });
  await page.reload();
  await expect(page.getByText('会话加载失败', { exact: true })).toBeVisible();
  await expect(page.getByText(/不支持此会话接口/)).toBeVisible();
  const initial = count;
  await page.waitForTimeout(6500);
  expect(count).toBe(initial);
  await page.getByRole('button', { name: '重试', exact: true }).click();
  await expect.poll(() => count).toBe(initial + 1);
  await page.waitForTimeout(3500);
  expect(count).toBe(initial + 1);
  fail = false;
  await page.getByRole('button', { name: '重试', exact: true }).click();
  await expect(page.getByText('会话加载失败', { exact: true })).toHaveCount(0);
  await expect.poll(() => count, { timeout: 6000 }).toBeGreaterThan(initial + 2);
});

test('model menu refreshes API cache, handles failure and fits mobile', async ({ page, request }) => {
  const before = catalogRequests;
  await request.post(`${endpoint}/settings/models/list`, { data: { provider: 'mock', refresh: true } });
  expect(catalogRequests).toBe(before + 1);
  await request.post(`${endpoint}/settings/models/list`, { data: { provider: 'mock' } });
  expect(catalogRequests).toBe(before + 1);
  let failing = true;
  let count = 0;
  await page.route('**/settings/models/list', async route => {
    count++;
    if (failing) await route.fulfill({ status: 502, contentType: 'application/json', body: '{"error":"模型服务不可用"}' });
    else await route.continue();
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole('button', { name: '选择模型', exact: true }).click();
  await expect(page.getByText('模型服务不可用', { exact: true })).toBeVisible();
  await page.waitForTimeout(3500);
  expect(count).toBe(1);
  failing = false;
  await page.getByRole('button', { name: '重试', exact: true }).click();
  await expect(page.getByText('模型服务不可用', { exact: true })).toHaveCount(0);
  const mockProvider = page.getByRole('button', { name: 'Provider 测试 Provider', exact: true });
  if (await mockProvider.getAttribute('aria-expanded') !== 'true') await mockProvider.click();
  const model = page.getByRole('button', { name: '模型 test-model', exact: true });
  if (await model.getAttribute('aria-expanded') !== 'true') await model.click();
  await expect(page.getByRole('button', { name: '思考等级 max', exact: true })).toBeVisible();
  const bounds = await page.getByRole('dialog', { name: '模型选择', exact: true }).boundingBox();
  expect(bounds!.x).toBeGreaterThanOrEqual(0);
  expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(390);
  await page.screenshot({ path: 'test-results/model-menu-mobile.png' });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.screenshot({ path: 'test-results/model-menu-desktop.png' });
  await page.getByRole('button', { name: '思考等级 max', exact: true }).click();
  await page.getByLabel('消息', { exact: true }).fill('思考等级验证');
  await page.getByLabel('消息', { exact: true }).press('Enter');
  await expect(page.locator('.chat-messages').getByText('已收到消息。', { exact: true })).toBeVisible();
  expect(requests.at(-1)?.reasoning).toEqual({ effort: 'max' });
});
