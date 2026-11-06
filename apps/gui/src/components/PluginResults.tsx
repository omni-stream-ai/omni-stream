import { useEffect, useState } from 'react';
import type { PluginResult } from '../lib/api';
import { useI18n } from '../i18n';
import { EmptyState } from './StateView';

function stringValue(value: unknown): string { return typeof value === 'string' ? value : ''; }
function richText(value: unknown): string {
  return Array.isArray(value) ? value.map(item => stringValue(item?.plain_text) || stringValue(item?.text?.content)).join('') : '';
}
function resultRow(value: unknown, index: number) {
  const item = (value && typeof value === 'object' ? value : {}) as Record<string, any>;
  const properties = Object.values(item.properties ?? {}) as Record<string, any>[];
  const title = stringValue(item.title) || richText(item.title) || stringValue(item.real_name) || stringValue(item.profile?.display_name) || stringValue(item.name) || stringValue(item.full_name) || richText(properties.find(property => property.type === 'title')?.title) || stringValue(item.message) || stringValue(item.message?.text) || stringValue(item.text) || richText(item[item.type]?.rich_text) || stringValue(item.id) || '#' + (index + 1);
  const url = stringValue(item.html_url) || stringValue(item.url);
  const description = stringValue(item.body) || stringValue(item.description) || stringValue(item.state) || (item.profile ? stringValue(item.id) : '');
  return { title, description, url: url.startsWith('https://') || url.startsWith('http://') ? url : '' };
}
export function PluginResults({ result }: { result: PluginResult }) {
  const { t } = useI18n();
  const [visibleCount, setVisibleCount] = useState(100);
  useEffect(() => setVisibleCount(100), [result]);
  const data = result.data as Record<string, any> | null;
  const list = Array.isArray(data) ? data : [data?.members, data?.channels, data?.messages, data?.results, data?.comments].find(Array.isArray);
  const rows = (list ?? (data ? [data.channel && typeof data.channel === 'object' ? data.channel : data.user && typeof data.user === 'object' ? data.user : data] : [])).map(resultRow);
  const raw = JSON.stringify(result, null, 2);
  const download = () => {
    const url = URL.createObjectURL(new Blob([raw], { type: 'application/json' }));
    const anchor = document.createElement('a'); anchor.href = url; anchor.download = 'plugin-result.json'; anchor.click(); window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  return <section className="plugin-results" aria-label={t('plugins.results')}>
    <div className="section-heading"><h4>{t('plugins.results')}</h4><span className="plugin-success" role="status">{t('plugins.operationSuccess')}</span></div>
    {!rows.length ? <EmptyState compact title={t('plugins.noData')} description={t('plugins.noDataHint')} /> : <><p className="muted plugin-token-note">{t('plugins.resultCount', { shown: Math.min(visibleCount, rows.length), total: rows.length })}</p><ul>{rows.slice(0, visibleCount).map((row, index) => <li key={index}>{row.url ? <a href={row.url} target="_blank" rel="noreferrer">{row.title}</a> : <strong>{row.title}</strong>}{row.description && <p>{row.description}</p>}</li>)}</ul>{visibleCount < rows.length && <button type="button" className="ghost" onClick={() => setVisibleCount(count => Math.min(count + 100, rows.length))}>{t('plugins.showMore', { count: rows.length - visibleCount })}</button>}</>}
    <details><summary>{t('plugins.rawResult')}</summary><pre>{raw.slice(0, 100000)}</pre>{raw.length > 100000 && <p>{t('plugins.previewTruncated')}</p>}</details>
    <button type="button" className="ghost" onClick={download}>{t('plugins.export')}</button>
  </section>;
}
