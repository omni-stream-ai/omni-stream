import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api, type NamedAgent } from '../lib/api';
import { AgentsPanel } from './AgentsPanel';

const agent: NamedAgent = { id: 1, name: '工作助手', description: 'Work', instructions: 'Check sources', engine: 'codex', plugins: null };
beforeEach(() => { vi.spyOn(api, 'agents').mockResolvedValue([]); vi.spyOn(api, 'agentEngines').mockResolvedValue([{ name: 'codex', protocol: 'app-server' }]); });
afterEach(() => { cleanup(); vi.restoreAllMocks(); });
function show() { render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })}><AgentsPanel server="fixture" /></QueryClientProvider>); }
it('creates an assistant inheriting current plugin permissions', async () => {
  const save = vi.spyOn(api, 'saveAgent').mockResolvedValue(agent);
  show();
  await screen.findByRole('option', { name: 'codex (app-server)' });
  fireEvent.change(screen.getByLabelText('Agent 名称'), { target: { value: '工作助手' } });
  fireEvent.change(screen.getByLabelText('角色指令'), { target: { value: 'Check sources' } });
  expect(screen.getByLabelText('沿用当前插件权限')).toBeChecked();
  fireEvent.click(screen.getByRole('button', { name: '创建 Agent' }));
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ name: '工作助手', instructions: 'Check sources', engine: 'codex', plugins: null }), undefined));
});
it('edits a saved assistant with an explicit plugin subset and deletes it', async () => {
  vi.mocked(api.agents).mockResolvedValue([agent]);
  const save = vi.spyOn(api, 'saveAgent').mockResolvedValue({ ...agent, plugins: ['slack'] });
  const remove = vi.spyOn(api, 'deleteAgent').mockResolvedValue({ deleted: true });
  show();
  fireEvent.click(await screen.findByRole('button', { name: '编辑 Agent' }));
  expect(screen.getByLabelText('Agent 名称')).toHaveValue('工作助手');
  fireEvent.click(screen.getByLabelText('沿用当前插件权限'));
  fireEvent.click(screen.getByLabelText('Slack'));
  fireEvent.click(screen.getByRole('button', { name: '保存' }));
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ plugins: ['slack'] }), 1));
  fireEvent.click(screen.getByRole('button', { name: '删除 Agent' }));
  await waitFor(() => expect(remove).toHaveBeenCalledWith(1, expect.anything()));
});
it('shows a failed list with a manual retry instead of an empty state', async () => {
  vi.mocked(api.agents).mockRejectedValueOnce(new Error('offline')).mockResolvedValue([agent]);
  show();
  expect(await screen.findByText('无法加载 Agent')).toBeInTheDocument();
  expect(screen.queryByText('还没有专属 Agent')).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: '重试' }));
  expect(await screen.findByText('@工作助手')).toBeInTheDocument();
});
it('loads memory on expansion, refreshes it and deletes the selected key', async () => {
  vi.mocked(api.agents).mockResolvedValue([agent]);
  const memory = { key: 'report-style', content: '简洁并附来源', created_at: 1, updated_at: 2, source: 'agent' };
  const list = vi.spyOn(api, 'agentMemories').mockResolvedValue([memory]);
  const remove = vi.spyOn(api, 'deleteAgentMemory').mockResolvedValue({ deleted: true });
  show();
  await screen.findByText('@工作助手');
  expect(list).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText('长期记忆', { exact: true }));
  expect(await screen.findByText('简洁并附来源')).toBeVisible();
  expect(list).toHaveBeenCalledWith(1);
  list.mockResolvedValue([{ ...memory, content: '更新后仍附来源' }]);
  fireEvent.click(screen.getByRole('button', { name: '刷新记忆' }));
  expect(await screen.findByText('更新后仍附来源')).toBeVisible();
  list.mockResolvedValue([]);
  fireEvent.click(screen.getByRole('button', { name: '删除记忆 report-style' }));
  await waitFor(() => expect(remove).toHaveBeenCalledWith(1, 'report-style'));
  expect(await screen.findByText('还没有长期记忆')).toBeVisible();
  expect(screen.queryByText('更新后仍附来源')).not.toBeInTheDocument();
});
it('shows memory read and delete failures without silently retrying', async () => {
  vi.mocked(api.agents).mockResolvedValue([agent]);
  const list = vi.spyOn(api, 'agentMemories').mockRejectedValueOnce(new Error('memory offline')).mockResolvedValue([{ key: 'keep', content: '保留此项', created_at: 1, updated_at: 2, source: 'agent' }]);
  const remove = vi.spyOn(api, 'deleteAgentMemory').mockRejectedValue(new Error('delete failed'));
  show();
  fireEvent.click(await screen.findByText('长期记忆', { exact: true }));
  expect(await screen.findByText('无法加载记忆')).toBeVisible();
  expect(screen.queryByText('还没有长期记忆')).not.toBeInTheDocument();
  expect(list).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole('button', { name: '重试' }));
  expect(await screen.findByText('保留此项')).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: '删除记忆 keep' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('delete failed');
  expect(screen.getByText('保留此项')).toBeVisible();
  expect(remove).toHaveBeenCalledTimes(1);
});
