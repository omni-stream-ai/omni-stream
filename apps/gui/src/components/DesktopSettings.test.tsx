import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { DesktopSettings } from './DesktopSettings';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), isTauri: vi.fn() }));
beforeEach(() => { localStorage.clear(); vi.mocked(isTauri).mockReturnValue(true); vi.mocked(invoke).mockResolvedValue({ allowLan: false, autostart: true }); });
afterEach(() => { cleanup(); vi.resetAllMocks(); });

it('does not expose host settings in the browser', () => {
  vi.mocked(isTauri).mockReturnValue(false);
  render(<DesktopSettings />);
  expect(screen.queryByRole('checkbox')).toBeNull();
  expect(invoke).not.toHaveBeenCalled();
});

it('reads system autostart and persists LAN opt-in with a restart notice', async () => {
  render(<DesktopSettings />);
  const lan = await screen.findByLabelText('允许局域网设备连接');
  expect(lan).not.toBeChecked();
  expect(screen.getByLabelText('开机自启')).toBeChecked();
  vi.mocked(invoke).mockResolvedValue(undefined);
  fireEvent.click(lan);
  await waitFor(() => expect(lan).toBeChecked());
  expect(invoke).toHaveBeenCalledWith('set_desktop_lan', { enabled: true });
  expect(screen.getByRole('status')).toHaveTextContent('重新启动');
  fireEvent.click(screen.getByLabelText('开机自启'));
  await waitFor(() => expect(screen.getByLabelText('开机自启')).not.toBeChecked());
  expect(invoke).toHaveBeenCalledWith('set_desktop_autostart', { enabled: false });
});

it('keeps the previous value when system registration fails', async () => {
  render(<DesktopSettings />);
  const autostart = await screen.findByLabelText('开机自启');
  vi.mocked(invoke).mockRejectedValue('registration failed');
  fireEvent.click(autostart);
  expect(await screen.findByRole('alert')).toHaveTextContent('registration failed');
  expect(autostart).toBeChecked();
});

it('allows retry after loading fails', async () => {
  vi.mocked(invoke).mockRejectedValueOnce('load failed');
  render(<DesktopSettings />);
  expect(await screen.findByRole('alert')).toHaveTextContent('load failed');
  fireEvent.click(screen.getByRole('button', { name: '重试' }));
  expect(await screen.findByLabelText('开机自启')).toBeChecked();
});
