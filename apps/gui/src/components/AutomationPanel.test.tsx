import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { AutomationPanel } from './AutomationPanel';
import { api, automationApi, pluginApi, type Automation } from '../lib/api';

beforeEach(() => { vi.spyOn(api, 'agents').mockResolvedValue([]); vi.spyOn(pluginApi, 'marketplace').mockResolvedValue([]); vi.spyOn(api, 'approvals').mockResolvedValue([]); });
afterEach(() => { cleanup(); vi.restoreAllMocks(); });
const item: Automation = { id: 3, title: '新闻日报', goal: '核实新闻来源', schedule: { kind: 'daily', hour: 9, minute: 0, utc_offset_minutes: 480 }, status: 'pending', next_run_at: null, conversation: 'inbox', runs: [] };
function show() {
  const onOpen = vi.fn();
  render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })}><AutomationPanel server="test" conversation="current-chat" onOpenConversation={onOpen} /></QueryClientProvider>);
  return onOpen;
}
it('creates a recurring delegation without requiring a project and binds delivery to the current chat', async () => {
  vi.spyOn(automationApi, 'list').mockResolvedValue([]);
  const create = vi.spyOn(automationApi, 'create').mockResolvedValue({ ...item, status: 'active' });
  show();
  fireEvent.change(screen.getByLabelText('委托内容'), { target: { value: '每天核实新闻来源' } });
  fireEvent.change(screen.getByLabelText('UTC 偏移（分钟）'), { target: { value: '480' } });
  fireEvent.click(screen.getByRole('button', { name: '创建并启用' }));
  await waitFor(() => expect(create).toHaveBeenCalledWith({ title: '每天核实新闻来源', goal: '每天核实新闻来源', agent_id: null, project: null, conversation: 'current-chat', schedule: { kind: 'daily', hour: 9, minute: 0, utc_offset_minutes: 480 } }));
});
it('shows pending drafts with explicit activation, independent history and a receiving conversation link', async () => {
  vi.spyOn(automationApi, 'list').mockResolvedValue([{ ...item, runs: [{ id: 1, started_at: 100, finished_at: 101, status: 'failed', output: '数据源不可访问' }] }]);
  const action = vi.spyOn(automationApi, 'action').mockResolvedValue({ ...item, status: 'active' });
  const onOpen = show();
  fireEvent.click(await screen.findByRole('button', { name: '确认启用' }));
  await waitFor(() => expect(action).toHaveBeenCalledWith(3, 'activate'));
  fireEvent.click(screen.getByText('执行记录 (1)'));
  expect(screen.getByText('数据源不可访问')).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: '打开接收会话' }));
  expect(onOpen).toHaveBeenCalledWith('inbox');
});
it('preserves the receiving conversation when editing an interval delegation', async () => {
  vi.spyOn(automationApi, 'list').mockResolvedValue([{ ...item, status: 'paused', schedule: { kind: 'interval', seconds: 3600 } }]);
  const update = vi.spyOn(automationApi, 'update').mockResolvedValue(item);
  show();
  fireEvent.click(await screen.findByRole('button', { name: '修改' }));
  fireEvent.change(screen.getByLabelText('间隔（分钟）'), { target: { value: '30' } });
  fireEvent.click(screen.getByRole('button', { name: '保存' }));
  await waitFor(() => expect(update).toHaveBeenCalledWith(3, expect.objectContaining({ conversation: 'inbox', schedule: { kind: 'interval', seconds: 1800 } })));
});
it('disables overlapping manual runs and exposes failed actions', async () => {
  vi.spyOn(automationApi, 'list').mockResolvedValue([{ ...item, status: 'active', runs: [{ id: 1, started_at: 100, finished_at: null, status: 'running', output: '' }] }]);
  vi.spyOn(automationApi, 'action').mockRejectedValue(new Error('暂停失败'));
  show();
  expect(await screen.findByRole('button', { name: '立即执行一次' })).toBeDisabled();
  expect(screen.getByRole('button', { name: '修改' })).toBeDisabled();
  fireEvent.click(screen.getByRole('button', { name: '暂停' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('暂停失败');
});

it('shows an accepted manual request as queued until the scheduler claims it', async () => {
  vi.spyOn(automationApi, 'list').mockResolvedValue([{ ...item, status: 'active', manual_run_at: 181, next_run_at: 220 }]);
  const action = vi.spyOn(automationApi, 'action').mockResolvedValue(item);
  show();
  expect(await screen.findByRole('button', { name: '排队中' })).toBeDisabled();
  expect(screen.queryByRole('button', { name: '立即执行一次' })).not.toBeInTheDocument();
  expect(action).not.toHaveBeenCalled();
});

it('preserves calendar, window and second precision when editing', async () => {
  vi.spyOn(automationApi, 'list').mockResolvedValue([{ ...item, schedule: { kind: 'daily', hour: 12, minute: 5, second: 17, utc_offset_minutes: 480 }, calendar: { weekdays: [1, 3], include_dates: [{ date: '2026-11-07', source: 'user', reason: 'extra' }], exclude_dates: [] }, time_window: { kind: 'previous_effective_day', hour: 12, minute: 0 }, allow_calendar_updates: true }]);
  const update = vi.spyOn(automationApi, 'update').mockResolvedValue(item);
  show();
  fireEvent.click(await screen.findByRole('button', { name: '修改' }));
  expect(screen.getByLabelText('执行时间')).toHaveValue('12:05:17');
  fireEvent.click(screen.getByText('日历、统计窗口与投递'));
  expect(screen.getByLabelText('周一')).toBeChecked();
  expect(screen.getByLabelText('周二')).not.toBeChecked();
  expect(screen.getByLabelText('日期')).toHaveValue('2026-11-07');
  fireEvent.click(screen.getByLabelText('周五'));
  fireEvent.click(screen.getByRole('button', { name: '保存' }));
  await waitFor(() => expect(update).toHaveBeenCalledWith(3, expect.objectContaining({ schedule: { kind: 'daily', hour: 12, minute: 5, second: 17, utc_offset_minutes: 480 }, calendar: expect.objectContaining({ weekdays: [1, 3, 5], include_dates: [{ date: '2026-11-07', source: 'user', reason: 'extra' }] }), time_window: { kind: 'previous_effective_day', hour: 12, minute: 0 }, allow_calendar_updates: true })));
});
it('configures only declared delivery actions with visible account and fixed destination', async () => {
  vi.mocked(pluginApi.marketplace).mockResolvedValue([{ manifest: { id: 'slack', name: 'Slack', version: '1', runtime: 'builtin', capabilities: [], permissions: [], actions: [] }, installed: true, installed_version: '1', connection: { status: 'connected', configured: true, enabled: true, allow_writes: true, account: 'fixture', verified_at: 1, last_error: null }, actions: [{ id: 'send_message', write: true, delivery_destination_fields: ['channel'], input_schema: { properties: { channel: { type: 'string' }, text: { type: 'string' } }, required: ['channel', 'text'] } }, { id: 'open_conversation', write: true, input_schema: { properties: {}, required: [] } }] }]);
  vi.spyOn(automationApi, 'list').mockResolvedValue([]);
  const create = vi.spyOn(automationApi, 'create').mockResolvedValue(item);
  show();
  fireEvent.change(screen.getByLabelText('委托内容'), { target: { value: 'Generate a summary' } });
  fireEvent.click(screen.getByText('日历、统计窗口与投递'));
  fireEvent.click(screen.getByRole('button', { name: '添加外部投递' }));
  await screen.findByRole('option', { name: 'Slack · fixture' });
  fireEvent.change(screen.getByLabelText('应用'), { target: { value: 'slack' } });
  expect(screen.queryByRole('option', { name: 'open_conversation' })).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText('动作'), { target: { value: 'send_message' } });
  fireEvent.change(screen.getByLabelText('channel · 固定目标'), { target: { value: 'C123' } });
  fireEvent.change(screen.getByLabelText('text · 投递内容'), { target: { value: '{{run.output_markdown}}' } });
  fireEvent.click(screen.getByRole('button', { name: '创建并启用' }));
  await waitFor(() => expect(create).toHaveBeenCalledWith(expect.objectContaining({ delivery_targets: [expect.objectContaining({ type: 'plugin_action', plugin: 'slack', action: 'send_message', destination: { channel: 'C123' }, payload: { text: '{{run.output_markdown}}' } })] })));
});
it('shows unknown delivery outcomes and revokes grants without running again', async () => {
  vi.spyOn(automationApi, 'list').mockResolvedValue([{ ...item, status: 'active', delivery_grants: [{ target_id: 'report', generation: 'revision', account: { account: 'fixture' }, granted_at: 1 }], runs: [{ id: 1, started_at: 1, finished_at: 2, status: 'failed', output: 'Delivery incomplete', deliveries: [{ target_id: 'report', idempotency_key: '1:1:report', status: 'unknown', attempts: 1, updated_at: 2, result: null, error: 'response lost' }] }] }]);
  const action = vi.spyOn(automationApi, 'action').mockResolvedValue(item);
  show();
  fireEvent.click(await screen.findByText('执行记录 (1)'));
  expect(screen.getByText(/结果不明，未重发/)).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: '撤销外部投递授权' }));
  await waitFor(() => expect(action).toHaveBeenCalledExactlyOnceWith(3, 'revoke'));
});

it('configures per-occurrence confirmation and preserves it during task edits', async () => {
  vi.spyOn(automationApi, 'list').mockResolvedValue([{ ...item, confirmation: { before_seconds: 1800, on_timeout: 'wait' } }]);
  const update = vi.spyOn(automationApi, 'update').mockResolvedValue(item);
  show();
  fireEvent.click(await screen.findByRole('button', { name: '修改' }));
  fireEvent.click(screen.getByText('日历、统计窗口与投递'));
  expect(screen.getByLabelText('启用逐次确认')).toBeChecked();
  expect(screen.getByLabelText('提前确认时间（秒）')).toHaveValue(1800);
  expect(screen.getByLabelText('到时未确认')).toHaveValue('wait');
  fireEvent.change(screen.getByLabelText('提前确认时间（秒）'), { target: { value: '600' } });
  fireEvent.change(screen.getByLabelText('到时未确认'), { target: { value: 'run' } });
  fireEvent.click(screen.getByRole('button', { name: '保存' }));
  await waitFor(() => expect(update).toHaveBeenCalledWith(3, expect.objectContaining({ confirmation: { before_seconds: 600, on_timeout: 'run' } })));
});
