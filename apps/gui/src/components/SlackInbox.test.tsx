import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { SlackInbox } from './SlackInbox';
import { slackApi, type SlackInboxState } from '../lib/api';
import { I18nProvider } from '../i18n';

const state: SlackInboxState = {
  connection: { status: 'connected', configured: true, enabled: true, auth_mode: 'slack_user', listen: true, listen_scope: 'related', allow_writes: true, account: 'personal-user', team_id: 'T123', user_id: 'U123', verified_at: 1, last_error: null },
  listener: { state: 'listening', error: null, connected_at: 1, last_event_at: 2 },
  conversations: [{ channel: 'D123', channel_type: 'im', ts: '1700000001.000001', text: 'Hello from Slack', count: 1 }],
};
const message = { id: 1, ts: '1700000001.000001', user: 'U456', text: 'Hello from Slack', own: false, deleted: false, edited: false, thread_ts: null };
function show() {
  const onConfigure = vi.fn();
  render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })}><I18nProvider><SlackInbox server="test-core" onConfigure={onConfigure} /></I18nProvider></QueryClientProvider>);
  return { onConfigure };
}
beforeEach(() => { localStorage.clear(); vi.spyOn(slackApi, 'inbox').mockResolvedValue(state); vi.spyOn(slackApi, 'messages').mockResolvedValue({ messages: [message], next_before: null }); });
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it('shows persisted messages and sends a thread reply with the reviewed personal identity', async () => {
  const send = vi.spyOn(slackApi, 'send').mockResolvedValue({ data: { ok: true }, next_cursor: null, next_page: null });
  const { onConfigure } = show();
  expect(await screen.findByText('正在接收新消息')).toBeVisible();
  const messages = await screen.findByRole('region', { name: 'Slack 消息' });
  expect(await within(messages).findByText('Hello from Slack', { exact: true })).toBeVisible();
  fireEvent.click(within(messages).getByRole('button', { name: '回复线程' }));
  fireEvent.change(screen.getByLabelText('Slack 消息内容'), { target: { value: 'Personal reply' } });
  fireEvent.click(screen.getByRole('button', { name: '发送到 Slack' }));
  expect(await screen.findByText('消息已发送到 Slack')).toBeVisible();
  expect(send).toHaveBeenCalledExactlyOnceWith({ team_id: 'T123', user_id: 'U123', channel: 'D123', text: 'Personal reply', thread_ts: message.ts });
  expect(screen.getByLabelText('Slack 消息内容')).toHaveValue('');
  fireEvent.click(screen.getByRole('button', { name: '配置与使用' })); expect(onConfigure).toHaveBeenCalledOnce();
});

it('keeps failed writes for manual correction and never retries them automatically', async () => {
  const send = vi.spyOn(slackApi, 'send').mockRejectedValue(new Error('Slack 身份已变化'));
  show();
  await screen.findByText('正在接收新消息');
  fireEvent.change(screen.getByLabelText('Slack 消息内容'), { target: { value: 'Keep this draft' } });
  fireEvent.click(screen.getByRole('button', { name: '发送到 Slack' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Slack 身份已变化');
  expect(screen.getByLabelText('Slack 消息内容')).toHaveValue('Keep this draft');
  expect(send).toHaveBeenCalledOnce();
});

it('disables sending when writes are off and retries a failed inbox load only on request', async () => {
  vi.mocked(slackApi.inbox).mockRejectedValueOnce(new Error('offline')).mockResolvedValue({ ...state, connection: { ...state.connection, allow_writes: false } });
  show();
  expect(await screen.findByRole('alert')).toHaveTextContent('offline');
  expect(slackApi.inbox).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole('button', { name: '重试' }));
  expect(await screen.findByText('正在接收新消息')).toBeVisible();
  await waitFor(() => expect(screen.getByLabelText('Slack 消息内容')).toBeDisabled());
  expect(screen.getByRole('button', { name: '发送到 Slack' })).toBeDisabled();
});
