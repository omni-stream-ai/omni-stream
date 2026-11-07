import type { Automation } from '../lib/api';
type Translate = (key: string, values?: Record<string, string | number>) => string;

export function scheduleLabel(item: Pick<Automation, 'schedule' | 'calendar'>, t: Translate) {
  const { schedule, calendar } = item;
  const weekdays = calendar?.weekdays?.length ? calendar.weekdays.map(day => t('automation.weekday' + day)).join('、') : '';
  const overrides = !!(calendar?.include_dates?.length || calendar?.exclude_dates?.length);
  let label: string;
  if (schedule.kind === 'interval') {
    label = schedule.seconds % 3600 === 0 ? t('automation.everyHours', { count: schedule.seconds / 3600 }) : t('automation.everyMinutes', { count: schedule.seconds / 60 });
    if (weekdays) label += ' · ' + weekdays + ' (UTC)';
  } else {
    const time = [schedule.hour, schedule.minute, ...(schedule.second ? [schedule.second] : [])].map(n => String(n).padStart(2, '0')).join(':');
    const offset = schedule.utc_offset_minutes;
    const zone = 'UTC' + (offset >= 0 ? '+' : '-') + String(Math.floor(Math.abs(offset) / 60)).padStart(2, '0') + ':' + String(Math.abs(offset) % 60).padStart(2, '0');
    label = (weekdays || t('automation.daily')) + ' · ' + time + ' ' + zone;
  }
  return label + (overrides ? ' · ' + t('automation.calendarAdjusted') : '');
}
