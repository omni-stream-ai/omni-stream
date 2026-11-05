import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { AutomationConfirmationCard } from './AutomationConfirmationCard';
import { automationApi, type AutomationConfirmation } from '../lib/api';

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
const review: AutomationConfirmation = { id: 1, scheduled_at: 1800000000, requested_at: 100, request_at: 100, deadline: null, trigger: 'manual', status: 'pending', goal: 'Original instructions', revision: 1, conversation: 'automation-confirmation:3:1', discussion_active: false, snapshot: { title: 'Digest', utc_offset_seconds: 28800, time_window: { start: 100, end: 200 }, delivery_targets: [{ type: 'plugin_action', id: 'report', plugin: 'slack', action: 'send_message', destination: { channel: 'C123' }, payload: { text: '{{run.output_markdown}}' } }], accounts: [{ target_id: 'report', account: { account: 'Reviewer account' } }] } };
function show(value = review, error?: Error) {
  vi.spyOn(automationApi, 'confirmation').mockImplementation(() => error ? Promise.reject(error) : Promise.resolve(value));
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(<MemoryRouter><QueryClientProvider client={client}><AutomationConfirmationCard server="test" automationId={3} confirmationId={1} /></QueryClientProvider></MemoryRouter>);
  return client;
}
it('requires saving edits before approving the exact revision and makes future edits explicit', async () => {
  const decide = vi.spyOn(automationApi, 'decide').mockResolvedValue({ ...review, goal: 'Revised instructions', revision: 2 });
  show();
  const editor = await screen.findByLabelText('本次委托内容');
  expect(screen.getByText(/Reviewer account/)).toBeVisible();
  expect(screen.getByRole('link', { name: '沟通调整' })).toHaveAttribute('href', '/chat/automation-confirmation%3A3%3A1');
  fireEvent.change(editor, { target: { value: 'Revised instructions' } });
  expect(screen.getByRole('button', { name: '确认本次执行' })).toBeDisabled();
  fireEvent.click(screen.getByRole('button', { name: '保存本次内容' }));
  await waitFor(() => expect(decide).toHaveBeenCalledWith(3, 1, 'update', { revision: 1, goal: 'Revised instructions' }));
  await waitFor(() => expect(screen.getByRole('button', { name: '确认本次执行' })).toBeEnabled());
  fireEvent.click(screen.getByLabelText('将本次内容用于后续执行'));
  fireEvent.click(screen.getByRole('button', { name: '确认本次执行' }));
  await waitFor(() => expect(decide).toHaveBeenCalledWith(3, 1, 'approve', { revision: 2, apply_to_future: true }));
});
it('preserves local edits when the Agent publishes a new revision and prevents stale approval', async () => {
  const client = show();
  const editor = await screen.findByLabelText('本次委托内容');
  fireEvent.change(editor, { target: { value: 'Unsaved local edits' } });
  act(() => client.setQueryData(['automation-confirmation', 'test', 3, 1], { ...review, revision: 2, goal: 'Agent revision' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('本地修改已保留');
  expect(editor).toHaveValue('Unsaved local edits');
  expect(screen.getByRole('button', { name: '保存本次内容' })).toBeDisabled();
  expect(screen.getByRole('button', { name: '确认本次执行' })).toBeDisabled();
  fireEvent.click(screen.getByRole('button', { name: '加载最新内容' }));
  expect(editor).toHaveValue('Agent revision');
  expect(screen.getByRole('button', { name: '确认本次执行' })).toBeEnabled();
});
it('blocks approval during discussion and shows terminal cards without approval actions', async () => {
  const client = show({ ...review, discussion_active: true });
  expect(await screen.findByRole('button', { name: '确认本次执行' })).toBeDisabled();
  expect(screen.getByRole('status')).toHaveTextContent('Agent 正在沟通调整');
  act(() => client.setQueryData(['automation-confirmation', 'test', 3, 1], { ...review, status: 'invalidated' }));
  expect(await screen.findByText('已失效')).toBeVisible();
  expect(screen.queryByRole('button', { name: '确认本次执行' })).not.toBeInTheDocument();
});
it('shows a failed fetch with a manual retry instead of an actionable empty card', async () => {
  show(review, new Error('Connection unavailable'));
  expect(await screen.findByText('Connection unavailable')).toBeVisible();
  expect(screen.queryByRole('button', { name: '确认本次执行' })).not.toBeInTheDocument();
  vi.mocked(automationApi.confirmation).mockResolvedValue(review);
  fireEvent.click(screen.getByRole('button', { name: '重试' }));
  expect(await screen.findByLabelText('本次委托内容')).toHaveValue('Original instructions');
});

it('explains automatic timeout execution and shows the frozen original content separately from a proposal', async () => {
  const client = show({ ...review, trigger: 'schedule', deadline: review.scheduled_at, snapshot: { ...review.snapshot, on_timeout: 'run', goal: 'Original instructions' } });
  expect(await screen.findByText('到时未确认则按原内容执行；已确认的修改才生效。正在讨论不会延迟执行。')).toBeVisible();
  act(() => client.setQueryData(['automation-confirmation', 'test', 3, 1], { ...review, trigger: 'schedule', status: 'ready', goal: 'Unapproved proposal', revision: 2, execution_source: 'timeout', execution_goal: 'Original instructions', snapshot: { ...review.snapshot, on_timeout: 'run' } }));
  expect(await screen.findByText('已到执行时间，等待执行')).toBeVisible();
  expect(screen.getByLabelText('本次委托内容')).toBeDisabled();
  expect(screen.queryByRole('button', { name: '确认本次执行' })).not.toBeInTheDocument();
  expect(screen.getByText('本次执行内容')).toBeVisible();
  expect(screen.getByText('Original instructions')).toBeVisible();
});
it('identifies automatic execution without labeling it as user approval', async () => {
  show({ ...review, status: 'consumed', execution_source: 'timeout', execution_goal: review.goal });
  expect(await screen.findByText('已按未确认策略触发')).toBeVisible();
  expect(screen.queryByText('已确认，等待执行')).not.toBeInTheDocument();
});
