import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { useViewedNotifications } from './useViewedNotifications';

let element: HTMLDivElement;
let visible = true;
let focused = true;
const snapshot = { scope: '["chat","one"]', instance: 'db', cursor: 10 };
beforeEach(() => {
  vi.useFakeTimers();
  element = document.createElement('div'); document.body.append(element);
  vi.spyOn(element, 'getBoundingClientRect').mockReturnValue({ top: 0, left: 0, bottom: 300, right: 400, width: 400, height: 300, x: 0, y: 0, toJSON() {} });
  for (const [key, value] of Object.entries({ scrollHeight: 300, scrollTop: 0, clientHeight: 300 })) Object.defineProperty(element, key, { configurable: true, writable: true, value });
  visible = true; focused = true;
  vi.spyOn(document, 'hasFocus').mockImplementation(() => focused);
  vi.spyOn(document, 'visibilityState', 'get').mockImplementation(() => visible ? 'visible' : 'hidden');
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => setTimeout(() => callback(0), 0));
  vi.stubGlobal('cancelAnimationFrame', clearTimeout);
});
afterEach(() => { cleanup(); element.remove(); document.querySelectorAll('dialog').forEach(item => item.remove()); vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers(); });
const settle = () => act(async () => { await vi.advanceTimersByTimeAsync(1); });

it('only reads successfully rendered foreground content and ignores background or unfocused views', async () => {
  const read = vi.fn().mockResolvedValue(undefined);
  const ref = { current: element };
  const { rerender } = renderHook(({ ready }) => useViewedNotifications(ref, snapshot, ready, read), { initialProps: { ready: false } });
  await settle(); expect(read).not.toHaveBeenCalled();
  visible = false; rerender({ ready: true }); await settle(); expect(read).not.toHaveBeenCalled();
  visible = true; focused = false; document.dispatchEvent(new Event('visibilitychange')); await settle(); expect(read).not.toHaveBeenCalled();
  focused = true; window.dispatchEvent(new Event('focus')); await settle(); expect(read).toHaveBeenCalledExactlyOnceWith(snapshot);
  window.dispatchEvent(new Event('focus')); await settle(); expect(read).toHaveBeenCalledOnce();
});

it('waits until the latest content is scrolled into view and never reads behind a modal', async () => {
  const read = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(element, 'scrollHeight', { value: 1200, configurable: true });
  renderHook(() => useViewedNotifications({ current: element }, snapshot, true, read));
  await settle(); expect(read).not.toHaveBeenCalled();
  const dialog = document.createElement('dialog'); dialog.setAttribute('open', ''); document.body.append(dialog);
  element.scrollTop = 900; element.dispatchEvent(new Event('scroll')); await settle(); expect(read).not.toHaveBeenCalled();
  dialog.remove(); await settle(); expect(read).toHaveBeenCalledExactlyOnceWith(snapshot);
});

it('advances only to the supplied content snapshot and cancels an unmounted view', async () => {
  const read = vi.fn().mockResolvedValue(undefined);
  const ref = { current: element };
  const { rerender, unmount } = renderHook(({ cursor }) => useViewedNotifications(ref, { ...snapshot, cursor }, true, read), { initialProps: { cursor: 10 } });
  await settle();
  rerender({ cursor: 12 }); await settle();
  expect(read.mock.calls.map(call => call[0].cursor)).toEqual([10, 12]);
  rerender({ cursor: 13 }); unmount(); await settle();
  expect(read).toHaveBeenCalledTimes(2);
});
