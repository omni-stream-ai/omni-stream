import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, expect, it, vi } from 'vitest';
import { AutomationList } from './AutomationList';
import { automationApi, type Automation } from '../lib/api';
import { scheduleLabel } from './automationDisplay';
import { translate } from '../i18n';

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
const base: Automation = { id: 1, title: '日报', goal: '核实来源，整理摘要', status: 'active', schedule: { kind: 'daily', hour: 9, minute: 0, utc_offset_minutes: 480 }, runs: [], next_run_at: 2000000000 };
function show() {
  const onOpen = vi.fn(), onCreate = vi.fn();
  render(<QueryClientProvider client={new QueryClient()}><AutomationList server="test" onOpen={onOpen} onCreate={onCreate} /></QueryClientProvider>);
  return { onOpen, onCreate };
}
it('prioritizes drafts, filters by status and opens the exact task without showing a form or execution controls', async () => {
  vi.spyOn(automationApi, 'list').mockResolvedValue([base, { ...base, id: 3, title: '已暂停任务', status: 'paused' }, { ...base, id: 2, title: '草案', status: 'pending', next_run_at: null }]);
  const { onOpen, onCreate } = show();
  const cards = await screen.findAllByRole('article');
  expect(within(cards[0]).getByText('草案')).toBeVisible();
  expect(screen.getByText('确认启用后安排')).toBeVisible();
  expect(screen.queryByLabelText('委托内容')).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '立即执行一次' })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: '查看 草案 的详情' }));
  expect(onOpen).toHaveBeenCalledWith(2);
  fireEvent.click(screen.getByRole('button', { name: '已暂停 1' }));
  expect(screen.getAllByRole('article')).toHaveLength(1);
  expect(screen.getByText('恢复后重新安排')).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: '新建长期委托' }));
  expect(onCreate).toHaveBeenCalledOnce();
});
it('keeps confirmation and run failures discoverable without expanding histories', async () => {
  vi.spyOn(automationApi, 'list').mockResolvedValue([{ ...base, confirmations: [{ id: 1, status: 'pending' }] as Automation['confirmations'] }, { ...base, id: 2, title: '失败任务', runs: [{ id: 1, started_at: 1, finished_at: 2, status: 'failed', output: 'private detailed failure' }] }]);
  show();
  expect(await screen.findByText('有待确认的执行')).toBeVisible();
  expect(screen.getByText('最近执行异常，请查看详情')).toBeVisible();
  expect(screen.queryByText('private detailed failure')).not.toBeInTheDocument();
});
it('distinguishes filtered empty results and failed requests with manual retry', async () => {
  const list = vi.spyOn(automationApi, 'list').mockRejectedValueOnce(new Error('offline')).mockResolvedValue([base]);
  show();
  expect(await screen.findByRole('alert')).toHaveTextContent('offline');
  expect(list).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole('button', { name: '重试' }));
  await screen.findByRole('article');
  fireEvent.click(screen.getByRole('button', { name: '已暂停 0' }));
  expect(screen.getByText('此状态下暂无任务')).toBeVisible();
  await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
});
it('summarizes calendar rules and offsets without listing every exception', () => {
  const item = { ...base, calendar: { weekdays: [1, 3], include_dates: [{ date: '2026-11-07', source: 'verified', reason: 'extra' }] } };
  const text = scheduleLabel(item, translate);
  expect(text).toContain('周一、周三');
  expect(text).toContain('09:00 UTC+08:00');
  expect(text).toContain('含日期调整');
  expect(text).not.toContain('2026-11-07');
  expect(scheduleLabel({ schedule: { kind: 'interval', seconds: 3600 } }, translate)).toBe('每 1 小时');
});
