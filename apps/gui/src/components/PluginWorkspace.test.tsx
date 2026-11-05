import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useState } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { PluginWorkspace } from './PluginWorkspace';
import { pluginApi, type MarketplacePlugin, type PluginAction, type PluginSetupOptions, type PluginSetupResult } from '../lib/api';
import { I18nProvider } from '../i18n';

const actions: PluginAction[] = [
  { id: 'list_repositories', write: false, input_schema: { properties: { per_page: { type: 'integer', minimum: 1, maximum: 100, default: 20 }, page: { type: 'integer', minimum: 1, maximum: 10000, default: 1 } }, required: [] } },
  { id: 'create_issue', write: true, input_schema: { properties: { owner: { type: 'string' }, repo: { type: 'string' }, title: { type: 'string' }, body: { type: 'string' } }, required: ['owner', 'repo', 'title'] } },
];
const plugin: MarketplacePlugin = { manifest: { id: 'github', name: 'GitHub', version: '0.1.0', runtime: 'builtin', capabilities: ['integration.github'], permissions: ['github.read', 'github.write'], actions: actions.map(action => action.id) }, installed: true, installed_version: '0.1.0', actions,
  connection: { status: 'unconfigured', configured: false, enabled: false, allow_writes: false, account: null, verified_at: null, last_error: null } };
const connected: MarketplacePlugin = { ...plugin, connection: { ...plugin.connection, configured: true, enabled: true, status: 'connected', account: 'octocat', verified_at: 100 } };
function show(initial = plugin, initialOptions?: PluginSetupOptions) {
  const onBack = vi.fn(); const onRefresh = vi.fn(); const onSetupStarted = vi.fn();
  let publish: (value: MarketplacePlugin) => void = () => undefined;
  function Harness() {
    const [value, setValue] = useState(initial);
    publish = setValue;
    return <PluginWorkspace plugin={value} initialOptions={initialOptions} onSetupStarted={onSetupStarted} onChange={setValue} onRefresh={onRefresh} onBack={onBack} />;
  }
  render(<QueryClientProvider client={new QueryClient({ defaultOptions: { mutations: { retry: false } } })}><I18nProvider><Harness /></I18nProvider></QueryClientProvider>);
  return { onBack, onRefresh, onSetupStarted, publish: (value: MarketplacePlugin) => act(() => publish(value)) };
}
beforeEach(() => localStorage.clear());
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it('starts a setup Agent with selected options, without submitting either draft credential', async () => {
  const slack: MarketplacePlugin = { ...plugin, manifest: { ...plugin.manifest, id: 'slack', name: 'Slack' } };
  let finish!: (value: PluginSetupResult) => void;
  const setup = vi.spyOn(pluginApi, 'setup').mockImplementation(() => new Promise(resolve => { finish = resolve; }));
  const connect = vi.spyOn(pluginApi, 'connect');
  const { onSetupStarted } = show(slack);
  expect(screen.getByText('手动配置说明').closest('details')).not.toHaveAttribute('open');
  fireEvent.change(screen.getByLabelText('Slack 连接方式'), { target: { value: 'slack_user' } });
  fireEvent.click(screen.getByLabelText('持续接收新消息'));
  fireEvent.change(screen.getByLabelText('监听范围'), { target: { value: 'all' } });
  fireEvent.click(screen.getByLabelText('允许写入此应用'));
  fireEvent.change(screen.getByLabelText('访问令牌'), { target: { value: 'xoxp-private-draft' } });
  fireEvent.change(screen.getByLabelText('App-level Token'), { target: { value: 'xapp-private-draft' } });
  fireEvent.click(screen.getByRole('button', { name: '让 Agent 协助获取令牌' }));
  expect(await screen.findByRole('button', { name: '正在启动 Agent…' })).toBeDisabled();
  expect(screen.getByLabelText('Slack 连接方式')).toBeDisabled();
  await waitFor(() => expect(setup).toHaveBeenCalledOnce());
  expect(setup.mock.calls[0]).toEqual(['slack', { auth_mode: 'slack_user', listen: true, listen_scope: 'all', enabled: true, allow_writes: true, locale: 'zh-CN' }, expect.stringMatching(/^[a-f0-9-]{36}$/)]);
  expect(JSON.stringify(setup.mock.calls)).not.toContain('private-draft');
  expect(connect).not.toHaveBeenCalled();
  finish({ conversation: 'plugin-setup:slack:fixture', call_id: 3 });
  await waitFor(() => expect(onSetupStarted).toHaveBeenCalledExactlyOnceWith('plugin-setup:slack:fixture'));
  expect(screen.getByLabelText('访问令牌')).toHaveValue('');
  expect(screen.getByLabelText('App-level Token')).toHaveValue('');
});

it('keeps manual setup usable on failure and reuses the request ID when retrying the same options', async () => {
  const setup = vi.spyOn(pluginApi, 'setup').mockRejectedValueOnce(new Error('请配置 config.toml 中的 Agent')).mockResolvedValue({ conversation: 'assistance', call_id: 4 });
  const { onSetupStarted } = show();
  fireEvent.change(screen.getByLabelText('访问令牌'), { target: { value: 'private-draft' } });
  fireEvent.click(screen.getByRole('button', { name: '让 Agent 协助获取令牌' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('config.toml');
  expect(onSetupStarted).not.toHaveBeenCalled();
  expect(setup).toHaveBeenCalledOnce();
  expect(screen.getByLabelText('访问令牌')).toHaveValue('private-draft');
  fireEvent.click(screen.getByText('手动配置说明'));
  expect(screen.getByRole('link', { name: '获取令牌说明' })).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: '让 Agent 协助获取令牌' }));
  await waitFor(() => expect(onSetupStarted).toHaveBeenCalledOnce());
  expect(setup.mock.calls[1]).toEqual(setup.mock.calls[0]);
});

it('restores only non-secret setup choices when returning from the Agent conversation', () => {
  const slack: MarketplacePlugin = { ...plugin, manifest: { ...plugin.manifest, id: 'slack', name: 'Slack' } };
  show(slack, { auth_mode: 'slack_user', listen: true, listen_scope: 'all', enabled: false, allow_writes: true, locale: 'zh-CN' });
  expect(screen.getByLabelText('Slack 连接方式')).toHaveValue('slack_user');
  expect(screen.getByLabelText('监听范围')).toHaveValue('all');
  expect(screen.getByLabelText('允许写入此应用')).toBeChecked();
  expect(screen.getByLabelText('启用此插件')).not.toBeChecked();
  expect(screen.getByLabelText('访问令牌')).toHaveValue('');
  expect(screen.getByLabelText('App-level Token')).toHaveValue('');
});

it('verifies and saves a token, clears the secret field and keeps it out of local storage', async () => {
  const connect = vi.spyOn(pluginApi, 'connect').mockResolvedValue(connected);
  const { onBack } = show();
  expect(screen.getByLabelText('访问令牌')).toHaveAttribute('type', 'password');
  expect(screen.getByRole('button', { name: '列出仓库' })).toBeDisabled();
  fireEvent.change(screen.getByLabelText('访问令牌'), { target: { value: 'private-fixture-token' } });
  fireEvent.click(screen.getByRole('button', { name: '验证并保存' }));
  expect(await screen.findByText('当前账号：octocat')).toBeVisible();
  expect(connect).toHaveBeenCalledExactlyOnceWith('github', { token: 'private-fixture-token', enabled: true, allow_writes: false });
  expect(screen.getByLabelText('访问令牌')).toHaveValue('');
  expect(JSON.stringify({ ...localStorage })).not.toContain('private-fixture-token');
  expect(screen.getByRole('button', { name: '列出仓库' })).toBeEnabled();
  fireEvent.click(screen.getByRole('button', { name: '返回插件市场' })); expect(onBack).toHaveBeenCalledOnce();
});

it('preserves the saved account after a failed replacement and lets the user verify, disable, and disconnect', async () => {
  const connect = vi.spyOn(pluginApi, 'connect').mockRejectedValueOnce(new Error('invalid token')).mockResolvedValueOnce({ ...connected, connection: { ...connected.connection, enabled: false, status: 'disabled' } });
  vi.spyOn(pluginApi, 'test').mockResolvedValue(connected);
  const disconnect = vi.spyOn(pluginApi, 'disconnect').mockResolvedValue(plugin);
  show(connected);
  fireEvent.change(screen.getByLabelText('访问令牌'), { target: { value: 'bad-token' } });
  fireEvent.click(screen.getByRole('button', { name: '验证并保存' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('invalid token');
  expect(screen.getByText('当前账号：octocat')).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: '测试连接' }));
  expect(await screen.findByText('连接验证成功')).toBeVisible();
  fireEvent.change(screen.getByLabelText('访问令牌'), { target: { value: '' } });
  fireEvent.click(screen.getByLabelText('启用此插件'));
  fireEvent.click(screen.getByRole('button', { name: '保存' }));
  await waitFor(() => expect(connect).toHaveBeenLastCalledWith('github', { token: undefined, enabled: false, allow_writes: false }));
  expect(await screen.findByText('已停用')).toBeVisible();
  expect(screen.getByRole('button', { name: '列出仓库' })).toBeDisabled();
  fireEvent.click(screen.getByRole('button', { name: '断开连接' }));
  expect(await screen.findByText('待连接')).toBeVisible();
  expect(screen.queryByText('当前账号：octocat')).not.toBeInTheDocument(); expect(disconnect).toHaveBeenCalledExactlyOnceWith('github');
});

it('renders real result links and requests the next page using the last executed parameters', async () => {
  const invoke = vi.spyOn(pluginApi, 'invoke').mockResolvedValueOnce({ data: [{ name: 'Repository one', html_url: 'https://github.com/org/one' }], next_page: 2, next_cursor: null }).mockResolvedValueOnce({ data: [], next_cursor: null, next_page: null });
  show(connected);
  fireEvent.click(screen.getByRole('button', { name: '列出仓库' }));
  expect(await screen.findByRole('link', { name: 'Repository one' })).toHaveAttribute('href', 'https://github.com/org/one');
  expect(invoke).toHaveBeenNthCalledWith(1, 'github', 'list_repositories', { per_page: 20, page: 1 });
  fireEvent.change(screen.getByLabelText('每页数量'), { target: { value: '50' } });
  fireEvent.click(screen.getByRole('button', { name: '下一页' }));
  expect(await screen.findByText('暂无结果')).toBeVisible();
  expect(invoke).toHaveBeenNthCalledWith(2, 'github', 'list_repositories', { per_page: 20, page: 2 });
  expect(screen.queryByRole('button', { name: '下一页' })).not.toBeInTheDocument();
});

it('requires saved write permission and does not retry a failed write automatically', async () => {
  vi.spyOn(pluginApi, 'connect').mockResolvedValue({ ...connected, connection: { ...connected.connection, allow_writes: true } });
  const invoke = vi.spyOn(pluginApi, 'invoke').mockRejectedValue(new Error('upstream timeout'));
  const { onRefresh } = show(connected);
  expect(screen.getByRole('option', { name: '创建 Issue · 写入' })).toBeDisabled();
  fireEvent.click(screen.getByLabelText('允许写入此应用'));
  fireEvent.click(screen.getByRole('button', { name: '验证并保存' }));
  await waitFor(() => expect(screen.getByRole('option', { name: '创建 Issue · 写入' })).toBeEnabled());
  fireEvent.change(screen.getByLabelText('选择操作'), { target: { value: 'create_issue' } });
  fireEvent.change(screen.getByLabelText(/仓库所有者/), { target: { value: 'org' } });
  fireEvent.change(screen.getByLabelText(/仓库名称/), { target: { value: 'project' } });
  fireEvent.change(screen.getByLabelText(/标题/), { target: { value: 'An issue' } });
  fireEvent.click(screen.getByRole('button', { name: '创建 Issue' }));
  const error = await screen.findByRole('alert'); expect(error).toHaveTextContent('upstream timeout'); expect(error).toHaveTextContent('避免重复写入');
  expect(invoke).toHaveBeenCalledExactlyOnceWith('github', 'create_issue', { owner: 'org', repo: 'project', title: 'An issue' });
  expect(onRefresh).toHaveBeenCalledOnce();
});

it('disables duplicate operation submissions while a service request is pending', async () => {
  let finish!: (value: { data: unknown; next_cursor: null; next_page: null }) => void;
  const invoke = vi.spyOn(pluginApi, 'invoke').mockImplementation(() => new Promise(resolve => { finish = resolve; }));
  show(connected);
  fireEvent.click(screen.getByRole('button', { name: '列出仓库' }));
  expect(await screen.findByRole('button', { name: '执行中…' })).toBeDisabled();
  expect(screen.getByLabelText('选择操作')).toBeDisabled();
  await waitFor(() => expect(invoke).toHaveBeenCalledOnce());
  finish({ data: [{ name: 'done' }], next_page: null, next_cursor: null });
  expect(await screen.findByText('done')).toBeVisible();
});

it('loads changed remote permissions without saving stale enable and write options', async () => {
  const disabled: MarketplacePlugin = { ...connected, connection: { ...connected.connection, enabled: false, allow_writes: false, status: 'disabled' } };
  const connect = vi.spyOn(pluginApi, 'connect').mockResolvedValue(disabled);
  const { publish } = show({ ...connected, connection: { ...connected.connection, allow_writes: true } });
  expect(screen.getByLabelText('启用此插件')).toBeChecked();
  expect(screen.getByLabelText('允许写入此应用')).toBeChecked();
  publish(disabled);
  await waitFor(() => expect(screen.getByLabelText('启用此插件')).not.toBeChecked());
  expect(screen.getByLabelText('允许写入此应用')).not.toBeChecked();
  expect(screen.getByText(/连接设置已更新/)).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: '保存' }));
  await waitFor(() => expect(connect).toHaveBeenCalledExactlyOnceWith('github', { token: undefined, enabled: false, allow_writes: false }));
});

it('expands every item in a long response without requesting a different service page', async () => {
  const data = Array.from({ length: 150 }, (_, index) => ({ name: 'Repository ' + index }));
  const invoke = vi.spyOn(pluginApi, 'invoke').mockResolvedValue({ data, next_cursor: null, next_page: null });
  show(connected);
  fireEvent.click(screen.getByRole('button', { name: '列出仓库' }));
  expect(await screen.findByText('显示 100 / 150 项')).toBeVisible();
  expect(screen.queryByText('Repository 149', { exact: true })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: '继续显示（剩余 50 项）' }));
  expect(screen.getByText('Repository 149', { exact: true })).toBeVisible();
  expect(screen.getByText('显示 150 / 150 项')).toBeVisible();
  expect(screen.queryByRole('button', { name: /继续显示/ })).not.toBeInTheDocument();
  expect(invoke).toHaveBeenCalledOnce();
});

it('clears previous results when credentials change even if the public account and verification time stay the same', async () => {
  vi.spyOn(pluginApi, 'invoke').mockResolvedValue({ data: [{ name: 'Previous connection data' }], next_cursor: null, next_page: null });
  vi.spyOn(pluginApi, 'connect').mockResolvedValue(connected);
  show(connected);
  fireEvent.change(screen.getByLabelText('每页数量'), { target: { value: '50' } });
  fireEvent.click(screen.getByRole('button', { name: '列出仓库' }));
  expect(await screen.findByText('Previous connection data')).toBeVisible();
  fireEvent.change(screen.getByLabelText('访问令牌'), { target: { value: 'replacement-fixture-token' } });
  fireEvent.click(screen.getByRole('button', { name: '验证并保存' }));
  expect(await screen.findByText('连接配置已保存')).toBeVisible();
  expect(screen.queryByText('Previous connection data')).not.toBeInTheDocument();
  expect(screen.getByLabelText('每页数量')).toHaveValue(20);
});

it('keeps the Slack bot connection while a user token is being configured, and saves the selected mode explicitly', async () => {
  const slack: MarketplacePlugin = { ...connected, manifest: { ...connected.manifest, id: 'slack', name: 'Slack' }, connection: { ...connected.connection, auth_mode: 'token', allow_writes: true } };
  const personal: MarketplacePlugin = { ...slack, connection: { ...slack.connection, auth_mode: 'slack_user', account: 'personal-user', allow_writes: false } };
  const connect = vi.spyOn(pluginApi, 'connect').mockRejectedValueOnce(new Error('User identity required')).mockResolvedValueOnce(personal);
  show(slack);
  expect(screen.getByLabelText('Slack 连接方式')).toHaveValue('token');
  fireEvent.change(screen.getByLabelText('访问令牌'), { target: { value: 'discard-this-draft' } });
  fireEvent.change(screen.getByLabelText('Slack 连接方式'), { target: { value: 'slack_user' } });
  expect(screen.getByLabelText('访问令牌')).toHaveValue('');
  expect(screen.getByLabelText('访问令牌')).toBeRequired();
  expect(screen.getByLabelText('允许写入此应用')).not.toBeChecked();
  expect(screen.getByRole('button', { name: '验证并保存' })).toBeDisabled();
  expect(screen.getByRole('button', { name: '测试连接' })).toBeDisabled();
  expect(screen.getByRole('button', { name: '列出仓库' })).toBeDisabled();
  fireEvent.click(screen.getByText('手动配置说明'));
  expect(screen.getByText(/无需邀请机器人到公共频道/)).toBeVisible();
  fireEvent.change(screen.getByLabelText('访问令牌'), { target: { value: 'xoxp-fixture-user' } });
  fireEvent.click(screen.getByRole('button', { name: '验证并保存' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('User identity required');
  expect(screen.getByText('当前账号：octocat')).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: '验证并保存' }));
  expect(await screen.findByText('当前账号：personal-user')).toBeVisible();
  expect(connect).toHaveBeenLastCalledWith('slack', { token: 'xoxp-fixture-user', auth_mode: 'slack_user', app_token: undefined, listen: false, listen_scope: 'related', enabled: true, allow_writes: false });
  expect(screen.getByLabelText('访问令牌')).toHaveValue('');
  expect(screen.getByLabelText('访问令牌')).not.toBeRequired();
  expect(screen.getByRole('button', { name: '测试连接' })).toBeEnabled();
  expect(JSON.stringify({ ...localStorage })).not.toContain('xoxp-fixture-user');
});

it('loads a remote Slack mode change and clears the previous operation results', async () => {
  const slack: MarketplacePlugin = { ...connected, manifest: { ...connected.manifest, id: 'slack', name: 'Slack' }, connection: { ...connected.connection, auth_mode: 'token' } };
  vi.spyOn(pluginApi, 'invoke').mockResolvedValue({ data: [{ name: 'Bot connection result' }], next_cursor: null, next_page: null });
  const { publish } = show(slack);
  fireEvent.click(screen.getByRole('button', { name: '列出仓库' }));
  expect(await screen.findByText('Bot connection result')).toBeVisible();
  publish({ ...slack, connection: { ...slack.connection, auth_mode: 'slack_user' } });
  await waitFor(() => expect(screen.getByLabelText('Slack 连接方式')).toHaveValue('slack_user'));
  expect(screen.queryByText('Bot connection result')).not.toBeInTheDocument();
  expect(screen.getByText(/连接设置已更新/)).toBeVisible();
});

it('lets users choose a continuous listening scope and protects both credentials', async () => {
  const slack: MarketplacePlugin = { ...plugin, manifest: { ...plugin.manifest, id: 'slack', name: 'Slack' } };
  const saved: MarketplacePlugin = { ...slack, connection: { ...connected.connection, auth_mode: 'slack_user', app_token_configured: true, listen: true, listen_scope: 'all', user_id: 'U123', team_id: 'T123' } };
  const connect = vi.spyOn(pluginApi, 'connect').mockResolvedValue(saved);
  show(slack);
  fireEvent.change(screen.getByLabelText('Slack 连接方式'), { target: { value: 'slack_user' } });
  fireEvent.change(screen.getByLabelText('访问令牌'), { target: { value: 'xoxp-private-user' } });
  fireEvent.click(screen.getByLabelText('持续接收新消息'));
  expect(screen.getByLabelText('监听范围')).toHaveValue('related');
  fireEvent.change(screen.getByLabelText('监听范围'), { target: { value: 'all' } });
  expect(screen.getByLabelText('App-level Token')).toHaveAttribute('type', 'password');
  expect(screen.getByRole('button', { name: '验证并保存' })).toBeDisabled();
  fireEvent.change(screen.getByLabelText('App-level Token'), { target: { value: 'xapp-private-app' } });
  fireEvent.click(screen.getByRole('button', { name: '验证并保存' }));
  expect(await screen.findByText('连接配置已保存')).toBeVisible();
  expect(connect).toHaveBeenCalledExactlyOnceWith('slack', { token: 'xoxp-private-user', auth_mode: 'slack_user', app_token: 'xapp-private-app', listen: true, listen_scope: 'all', enabled: true, allow_writes: false });
  expect(screen.getByLabelText('访问令牌')).toHaveValue('');
  expect(screen.getByLabelText('App-level Token')).toHaveValue('');
  expect(screen.getByLabelText('App-level Token')).not.toBeRequired();
  expect(JSON.stringify({ ...localStorage })).not.toContain('private-');
});


it('shows Slack member names and IDs and keeps the directory cursor for the next page', async () => {
  const member = { id: 'U789', name: 'junjie', real_name: 'JunJie Wang', profile: { display_name: 'JunJie' } };
  const invoke = vi.spyOn(pluginApi, 'invoke')
    .mockResolvedValueOnce({ data: { members: [member] }, next_cursor: 'members-page-2', next_page: null })
    .mockResolvedValueOnce({ data: { members: [] }, next_cursor: null, next_page: null })
    .mockResolvedValueOnce({ data: { user: member }, next_cursor: null, next_page: null });
  show({ ...connected, manifest: { ...connected.manifest, id: 'slack', name: 'Slack' }, actions: [
    { id: 'list_users', write: false, input_schema: { properties: { limit: { type: 'integer', default: 100 }, cursor: { type: 'string' } }, required: [] } },
    { id: 'user_info', write: false, input_schema: { properties: { user: { type: 'string' } }, required: ['user'] } },
  ] });
  fireEvent.click(screen.getByRole('button', { name: '列出成员' }));
  expect(await screen.findByText('JunJie Wang')).toBeVisible();
  expect(screen.getByText('U789')).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: '下一页' }));
  expect(await screen.findByText('暂无结果')).toBeVisible();
  expect(invoke).toHaveBeenNthCalledWith(2, 'slack', 'list_users', { limit: 100, cursor: 'members-page-2' });
  fireEvent.change(screen.getByLabelText('选择操作'), { target: { value: 'user_info' } });
  fireEvent.change(screen.getByLabelText(/用户 ID/), { target: { value: 'U789' } });
  fireEvent.click(screen.getByRole('button', { name: '读取成员资料' }));
  expect(await screen.findByText('JunJie Wang')).toBeVisible();
  expect(invoke).toHaveBeenNthCalledWith(3, 'slack', 'user_info', { user: 'U789' });
});
