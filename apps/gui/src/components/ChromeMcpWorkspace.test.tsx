import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useState } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ChromeMcpWorkspace } from './ChromeMcpWorkspace';
import { pluginApi, type MarketplacePlugin } from '../lib/api';
import { I18nProvider } from '../i18n';

const plugin: MarketplacePlugin = { manifest: { id: 'chrome-devtools', name: 'Chrome DevTools MCP', version: '1.12.0', runtime: 'mcp', capabilities: ['browser.automation'], permissions: ['browser.control'], actions: [] }, installed: true, installed_version: '1.12.0', actions: [], connection: { status: 'unconfigured', configured: false, enabled: false, allow_writes: false, account: null, verified_at: null, last_error: null }, browser: { enabled: false, status: 'disabled', package_version: '1.12.0', verified_at: null, last_error: null } };
const enabled: MarketplacePlugin = { ...plugin, browser: { ...plugin.browser!, enabled: true, status: 'unverified' } };
function show(initial = plugin) {
  const onRefresh = vi.fn(); const onBack = vi.fn();
  function Harness() { const [value, setValue] = useState(initial); return <ChromeMcpWorkspace plugin={value} onBack={onBack} onChange={setValue} onRefresh={onRefresh} />; }
  render(<QueryClientProvider client={new QueryClient()}><I18nProvider><Harness /></I18nProvider></QueryClientProvider>);
  return { onRefresh, onBack };
}
beforeEach(() => localStorage.clear());
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it('lets users explicitly enable browser tools without a service token form', async () => {
  const save = vi.spyOn(pluginApi, 'setBrowserEnabled').mockResolvedValue(enabled);
  const { onBack } = show();
  expect(screen.queryByLabelText('访问令牌')).not.toBeInTheDocument();
  expect(screen.getByRole('button', { name: '检查连接' })).toBeDisabled();
  expect(screen.getByLabelText('允许 Agent 使用 Chrome 浏览器')).not.toBeChecked();
  fireEvent.click(screen.getByLabelText('允许 Agent 使用 Chrome 浏览器'));
  fireEvent.click(screen.getByRole('button', { name: '保存' }));
  expect(await screen.findByText('浏览器工具设置已保存')).toBeVisible();
  expect(save).toHaveBeenCalledExactlyOnceWith(true);
  expect(screen.getByRole('button', { name: '检查连接' })).toBeEnabled();
  fireEvent.click(screen.getByRole('button', { name: '返回插件市场' }));
  expect(onBack).toHaveBeenCalledOnce();
});

it('checks the real connection and prevents duplicate requests while waiting for browser approval', async () => {
  let finish!: (value: MarketplacePlugin) => void;
  const test = vi.spyOn(pluginApi, 'test').mockImplementation(() => new Promise(resolve => { finish = resolve; }));
  show(enabled);
  fireEvent.click(screen.getByRole('button', { name: '检查连接' }));
  expect(await screen.findByRole('button', { name: '正在检查…' })).toBeDisabled();
  expect(screen.getByRole('button', { name: '保存' })).toBeDisabled();
  await waitFor(() => expect(test).toHaveBeenCalledExactlyOnceWith('chrome-devtools'));
  finish({ ...enabled, browser: { ...enabled.browser!, status: 'ready', verified_at: 100 } });
  expect(await screen.findByText('Chrome 浏览器连接正常')).toBeVisible();
});

it('shows actionable browser errors without retrying or reporting an unverified connection as ready', async () => {
  const test = vi.spyOn(pluginApi, 'test').mockRejectedValue(new Error('请在 Chrome 中允许连接'));
  const { onRefresh } = show(enabled);
  fireEvent.click(screen.getByRole('button', { name: '检查连接' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('请在 Chrome 中允许连接');
  expect(test).toHaveBeenCalledOnce(); expect(onRefresh).toHaveBeenCalledOnce();
  expect(screen.queryByText('Chrome 浏览器连接正常')).not.toBeInTheDocument();
  expect(screen.getByRole('button', { name: '检查连接' })).toBeEnabled();
});
