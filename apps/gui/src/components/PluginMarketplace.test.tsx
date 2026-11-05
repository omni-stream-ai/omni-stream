import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { PluginMarketplace } from './PluginMarketplace';
import { pluginApi, type MarketplacePlugin } from '../lib/api';
import { I18nProvider } from '../i18n';

const plugins: MarketplacePlugin[] = ['Slack', 'GitHub', 'Notion', 'Figma'].map(name => ({
  manifest: { id: name.toLowerCase(), name, version: '0.1.0', runtime: 'manifest', capabilities: [`integration.${name.toLowerCase()}`], permissions: [], actions: [] },
  installed: false, installed_version: null,
  connection: { status: 'unconfigured', configured: false, enabled: false, allow_writes: false, account: null, verified_at: null, last_error: null }, actions: [],
}));
const installed = (item: MarketplacePlugin) => ({ ...item, installed: true, installed_version: item.manifest.version });
function show(server = 'test-core') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } } });
  render(<QueryClientProvider client={client}><I18nProvider><PluginMarketplace server={server} /></I18nProvider></QueryClientProvider>);
  return client;
}
beforeEach(() => { localStorage.clear(); vi.spyOn(pluginApi, 'marketplace').mockResolvedValue(plugins); });
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it('leaves all four apps uninstalled until selected and filters by name or description', async () => {
  const install = vi.spyOn(pluginApi, 'install').mockResolvedValue(installed(plugins[0]));
  show();
  expect(await screen.findAllByRole('article')).toHaveLength(4);
  expect(install).not.toHaveBeenCalled();
  expect(screen.getByText(/下载后连接账号/)).toBeVisible();
  fireEvent.change(screen.getByRole('textbox', { name: '搜索插件…' }), { target: { value: '  GITHUB  ' } });
  expect(screen.getAllByRole('article')).toHaveLength(1);
  expect(screen.getByRole('article', { name: 'GitHub' })).toBeVisible();
  fireEvent.change(screen.getByRole('textbox'), { target: { value: '知识库' } });
  expect(screen.getByRole('article', { name: 'Notion' })).toBeVisible();
  fireEvent.change(screen.getByRole('textbox'), { target: { value: 'unknown' } });
  expect(screen.getByText('未找到匹配的插件')).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: '浏览全部插件' }));
  expect(screen.getAllByRole('article')).toHaveLength(4);
});

it('prevents duplicate installs, persists the returned state in the Core-specific cache, and uninstalls', async () => {
  let finish!: (value: MarketplacePlugin) => void;
  const install = vi.spyOn(pluginApi, 'install').mockImplementation(() => new Promise(resolve => { finish = resolve; }));
  const uninstall = vi.spyOn(pluginApi, 'uninstall').mockResolvedValue(plugins[0]);
  const client = show();
  const slack = within(await screen.findByRole('article', { name: 'Slack' }));
  fireEvent.click(slack.getByRole('button', { name: '下载' }));
  expect(await slack.findByRole('button', { name: '下载中…' })).toBeDisabled();
  expect(within(screen.getByRole('article', { name: 'GitHub' })).getByRole('button', { name: '下载' })).toBeEnabled();
  await waitFor(() => expect(install).toHaveBeenCalledExactlyOnceWith('slack'));
  finish(installed(plugins[0]));
  expect(await slack.findByRole('button', { name: '卸载' })).toBeEnabled();
  expect(client.getQueryData<MarketplacePlugin[]>(['plugin-marketplace', 'test-core'])?.[0].installed).toBe(true);
  expect(client.getQueryData(['plugin-marketplace', 'other-core'])).toBeUndefined();
  fireEvent.click(screen.getByRole('button', { name: '已安装 1' }));
  expect(screen.getAllByRole('article')).toHaveLength(1);
  fireEvent.click(slack.getByRole('button', { name: '卸载' }));
  expect(await screen.findByText('还没有安装插件')).toBeVisible();
  expect(uninstall).toHaveBeenCalledExactlyOnceWith('slack');
});

it('keeps a failed action visible for manual retry without marking the plugin installed', async () => {
  const install = vi.spyOn(pluginApi, 'install').mockRejectedValueOnce(new Error('disk unavailable')).mockResolvedValueOnce(installed(plugins[1]));
  show();
  const github = within(await screen.findByRole('article', { name: 'GitHub' }));
  fireEvent.click(github.getByRole('button', { name: '下载' }));
  expect(await github.findByRole('alert')).toHaveTextContent('disk unavailable');
  expect(github.queryByText('已安装')).not.toBeInTheDocument();
  expect(install).toHaveBeenCalledTimes(1);
  fireEvent.click(github.getByRole('button', { name: '重试' }));
  expect(await github.findByText('已安装')).toBeVisible();
  expect(github.queryByRole('alert')).not.toBeInTheDocument();
});

it('shows loading and failure separately from empty states and retries only on request', async () => {
  let fail!: (error: Error) => void;
  vi.mocked(pluginApi.marketplace).mockImplementationOnce(() => new Promise((_, reject) => { fail = reject; }));
  show();
  expect(screen.getByRole('status')).toHaveTextContent('加载中');
  expect(screen.queryByText('还没有安装插件')).not.toBeInTheDocument();
  fail(new Error('Core offline'));
  expect(await screen.findByRole('alert')).toHaveTextContent('Core offline');
  expect(pluginApi.marketplace).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole('button', { name: '重试' }));
  expect(await screen.findAllByRole('article')).toHaveLength(4);
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  expect(pluginApi.marketplace).toHaveBeenCalledTimes(2);
});

it('preserves installed state when uninstall fails and retries the same operation', async () => {
  vi.mocked(pluginApi.marketplace).mockResolvedValue([installed(plugins[2])]);
  const uninstall = vi.spyOn(pluginApi, 'uninstall').mockRejectedValueOnce(new Error('read only')).mockResolvedValueOnce(plugins[2]);
  show();
  const notion = within(await screen.findByRole('article', { name: 'Notion' }));
  fireEvent.click(notion.getByRole('button', { name: '卸载' }));
  expect(await notion.findByRole('alert')).toHaveTextContent('read only');
  expect(notion.getByText('已安装')).toBeVisible();
  fireEvent.click(notion.getByRole('button', { name: '重试' }));
  expect(await notion.findByRole('button', { name: '下载' })).toBeEnabled();
  expect(uninstall).toHaveBeenNthCalledWith(2, 'notion');
});

it('renders the marketplace and actions in English', async () => {
  localStorage.setItem('omnistream.locale', 'en-US');
  show();
  expect(await screen.findAllByRole('button', { name: 'Download' })).toHaveLength(4);
  fireEvent.click(screen.getByRole('button', { name: 'Installed 0' }));
  expect(screen.getByText('No plugins installed yet')).toBeVisible();
});

it('opens Chrome browser configuration without showing service credential fields', async () => {
  const chrome: MarketplacePlugin = { ...plugins[0], manifest: { ...plugins[0].manifest, id: 'chrome-devtools', name: 'Chrome DevTools MCP', runtime: 'mcp' }, installed: true, installed_version: '1.12.0', browser: { enabled: false, status: 'disabled', package_version: '1.12.0', verified_at: null, last_error: null } };
  vi.mocked(pluginApi.marketplace).mockResolvedValue([...plugins, chrome]);
  show();
  const card = within(await screen.findByRole('article', { name: 'Chrome DevTools MCP' }));
  fireEvent.click(card.getByRole('button', { name: '配置与使用' }));
  expect(await screen.findByRole('heading', { name: 'Chrome 浏览器自动化' })).toBeVisible();
  expect(screen.getByLabelText('允许 Agent 使用 Chrome 浏览器')).not.toBeChecked();
  expect(screen.queryByLabelText('访问令牌')).not.toBeInTheDocument();
});
