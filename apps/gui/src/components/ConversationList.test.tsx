import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useRef } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ConversationList } from './ConversationList';
import { api, type ConversationPage } from '../lib/api';
import { I18nProvider } from '../i18n';

const first: ConversationPage = {
  items: Array.from({ length: 20 }, (_, index) => ({ id: `session-${44 - index}`, title: `Title ${44 - index}`, updated_at: 100 - index })),
  next_cursor: { id: 'session-25', updated_at: 81 },
  total: 45,
};
const second: ConversationPage = {
  items: Array.from({ length: 20 }, (_, index) => ({ id: `session-${24 - index}`, title: `Title ${24 - index}`, updated_at: 80 - index })),
  next_cursor: { id: 'session-5', updated_at: 61 },
  total: 45,
};
const last: ConversationPage = {
  items: Array.from({ length: 5 }, (_, index) => ({ id: `session-${4 - index}`, title: `Title ${4 - index}`, updated_at: 60 - index })),
  next_cursor: null,
  total: 45,
};
let observers: { callback: IntersectionObserverCallback; options?: IntersectionObserverInit; target?: Element; disconnected: boolean }[];

function show() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false } } });
  const onSelect = vi.fn();
  function List() {
    const scroll = useRef<HTMLDivElement>(null);
    return <div className="sidebar-scroll" ref={scroll}><ConversationList server="test-core" conversation="session-44" active enabled scrollContainer={scroll} onSelect={onSelect} onAction={vi.fn()} /></div>;
  }
  const result = render(<QueryClientProvider client={client}><I18nProvider><List /></I18nProvider></QueryClientProvider>);
  return { ...result, client, onSelect };
}
function intersect() {
  const observer = observers.findLast(item => !item.disconnected);
  if (observer) act(() => observer.callback([{ isIntersecting: true, target: observer.target } as IntersectionObserverEntry], {} as IntersectionObserver));
}
beforeEach(() => {
  localStorage.clear();
  observers = [];
  vi.stubGlobal('IntersectionObserver', class {
    state: typeof observers[number];
    constructor(callback: IntersectionObserverCallback, options?: IntersectionObserverInit) {
      this.state = { callback, options, disconnected: false };
      observers.push(this.state);
    }
    observe(target: Element) { this.state.target = target; }
    disconnect() { this.state.disconnected = true; }
  });
  vi.spyOn(api, 'conversationDetail').mockResolvedValue({ exists: true, history: [], calls: [], settings: { approval_mode: 'default' } });
  vi.spyOn(api, 'conversation');
  vi.spyOn(api, 'conversationSettings');
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it('loads 20 summaries at a time inside the sidebar without duplicate requests or per-session history loads', async () => {
  let finish!: (page: ConversationPage) => void;
  const list = vi.spyOn(api, 'conversations').mockResolvedValueOnce(first).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; })).mockResolvedValueOnce(last);
  const { container, onSelect } = show();
  await screen.findByRole('button', { name: 'Title 44' });
  await waitFor(() => expect(observers.some(item => !item.disconnected)).toBe(true));
  expect(container.querySelectorAll('.conversation-list-entry')).toHaveLength(20);
  expect(screen.queryByRole('button', { name: 'Title 24' })).not.toBeInTheDocument();
  expect(observers.at(-1)?.options?.root).toBe(container.querySelector('.sidebar-scroll'));
  expect(container.querySelector('.count')).toHaveTextContent('45');
  expect(list).toHaveBeenCalledExactlyOnceWith(null);
  act(() => { intersect(); intersect(); });
  expect(await screen.findByRole('status', { name: '正在加载更多会话' })).toBeVisible();
  expect(list).toHaveBeenCalledTimes(2);
  expect(list).toHaveBeenLastCalledWith(first.next_cursor);
  await act(async () => finish(second));
  await screen.findByRole('button', { name: 'Title 24' });
  await waitFor(() => expect(container.querySelectorAll('.conversation-list-entry')).toHaveLength(40));
  intersect();
  await screen.findByRole('button', { name: 'Title 0' });
  expect(container.querySelectorAll('.conversation-list-entry')).toHaveLength(45);
  expect(list).toHaveBeenLastCalledWith(second.next_cursor);
  await waitFor(() => expect(observers.every(item => item.disconnected)).toBe(true));
  intersect();
  expect(list).toHaveBeenCalledTimes(3);
  expect(api.conversation).not.toHaveBeenCalled();
  expect(api.conversationSettings).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Title 24' }));
  expect(onSelect).toHaveBeenCalledWith('session-24');
});

it('keeps existing rows after a page failure and retries that cursor only on manual request', async () => {
  let finish!: (page: ConversationPage) => void;
  const list = vi.spyOn(api, 'conversations').mockResolvedValueOnce(first).mockRejectedValueOnce(new Error('offline')).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  const { container } = show();
  await screen.findByRole('button', { name: 'Title 44' });
  await waitFor(() => expect(observers.some(item => !item.disconnected)).toBe(true));
  intersect();
  expect(await screen.findByRole('alert')).toHaveTextContent('暂时无法获取会话列表');
  expect(container.querySelectorAll('.conversation-list-entry')).toHaveLength(20);
  await waitFor(() => expect(observers.every(item => item.disconnected)).toBe(true));
  intersect();
  fireEvent(window, new Event('focus'));
  fireEvent(window, new Event('online'));
  expect(list).toHaveBeenCalledTimes(2);
  fireEvent.click(screen.getByRole('button', { name: '重试' }));
  await waitFor(() => expect(screen.getByRole('button', { name: '加载中...' })).toBeDisabled());
  expect(list).toHaveBeenCalledTimes(3);
  expect(list).toHaveBeenLastCalledWith(first.next_cursor);
  await act(async () => finish({ ...second, next_cursor: null }));
  await screen.findByRole('button', { name: 'Title 24' });
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  expect(container.querySelectorAll('.conversation-list-entry')).toHaveLength(40);
});

it('retries a failed first page manually without showing a persisted session as a new draft', async () => {
  const list = vi.spyOn(api, 'conversations').mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce({ ...first, next_cursor: null });
  show();
  await screen.findByRole('alert');
  expect(screen.queryByRole('button', { name: '新会话' })).not.toBeInTheDocument();
  expect(list).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole('button', { name: '重试' }));
  await screen.findByRole('button', { name: 'Title 44' });
  expect(list).toHaveBeenCalledTimes(2);
  expect(list).toHaveBeenLastCalledWith(null);
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

it('shows an unsaved conversation above the summaries without counting it twice after persistence', async () => {
  vi.mocked(api.conversationDetail).mockResolvedValueOnce({ exists: false, history: [], calls: [], settings: { approval_mode: 'default' } });
  vi.spyOn(api, 'conversations').mockResolvedValueOnce({ ...first, items: [], total: 0, next_cursor: null }).mockResolvedValueOnce({ ...first, next_cursor: null });
  const { client, container } = show();
  expect(await screen.findByRole('button', { name: '新会话' })).toHaveAttribute('aria-current', 'page');
  await waitFor(() => expect(container.querySelector('.count')).toHaveTextContent('1'));
  await act(async () => { await client.invalidateQueries({ queryKey: ['conversations', 'test-core'] }); });
  expect(await screen.findByRole('button', { name: 'Title 44' })).toHaveAttribute('aria-current', 'page');
  expect(screen.queryByRole('button', { name: '新会话' })).not.toBeInTheDocument();
  expect(container.querySelector('.count')).toHaveTextContent('45');
});
