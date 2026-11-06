import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, Chrome, Download, Figma, Github, LayoutGrid, LoaderCircle, Search, Slack, Trash2, Settings2 } from 'lucide-react';
import { pluginApi, type MarketplacePlugin, type PluginSetupContext } from '../lib/api';
import { useI18n } from '../i18n';
import { EmptyState, ErrorState, LoadingState } from './StateView';
import { PluginWorkspace } from './PluginWorkspace';
import { ChromeMcpWorkspace } from './ChromeMcpWorkspace';
import './PluginMarketplace.css';

function PluginCard({ item, queryKey, busy, onManage }: { item: MarketplacePlugin; queryKey: readonly string[]; busy: boolean; onManage: () => void }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const { id, name, version } = item.manifest;
  const change = useMutation({
    mutationFn: (install: boolean) => install ? pluginApi.install(id) : pluginApi.uninstall(id),
    retry: false,
    onSuccess: updated => {
      client.setQueryData<MarketplacePlugin[]>(queryKey, current => current?.map(plugin => plugin.manifest.id === id ? updated : plugin));
    },
  });
  const Icon = { slack: Slack, github: Github, notion: LayoutGrid, figma: Figma, 'chrome-devtools': Chrome }[id] ?? LayoutGrid;
  const action = change.isPending ? (change.variables ? 'plugins.installing' : 'plugins.uninstalling') : item.installed ? 'plugins.uninstall' : 'plugins.install';
  const retryAction = () => change.mutate(change.variables ?? !item.installed);
  return <article className="plugin-card" aria-label={name} aria-busy={change.isPending}>
    <div className="plugin-card-heading">
      <span className={`plugin-icon plugin-icon-${id}`} aria-hidden="true">{id === 'notion' ? <span className="notion-mark">N</span> : <Icon size={26} />}</span>
      <div><h3>{name}</h3><span className="muted plugin-version">v{item.installed_version ?? version}</span></div>
      {item.installed && <span className="plugin-installed"><Check size={13} />{t('plugins.installed')}</span>}
    </div>
    <p className="plugin-description">{t(`plugins.${id}.description`)}</p>
    {item.installed && <p className={'plugin-connection-status ' + item.connection.status}>{t(item.browser ? 'chrome.state.' + item.browser.status : 'plugins.connection.' + item.connection.status)}{item.connection.account ? ' · ' + item.connection.account : ''}</p>}
    <div className="plugin-card-footer">
      <span className="plugin-category">{t(`plugins.${id}.category`)}</span>
      <button type="button" className={item.installed ? 'ghost' : 'primary'} disabled={change.isPending || busy} onClick={() => change.mutate(!item.installed)}>
        {change.isPending ? <LoaderCircle size={14} className="spin" /> : item.installed ? <Trash2 size={14} /> : <Download size={14} />}
        {t(action)}
      </button>
      {item.installed && <button type="button" className="primary" disabled={change.isPending || busy} onClick={onManage}><Settings2 size={14} />{t('plugins.manage')}</button>}
    </div>
    {change.error && <ErrorState compact title={t('plugins.actionFailed', { name })} description={change.error.message} onRetry={retryAction} retrying={change.isPending || busy} />}
  </article>;
}

export function PluginMarketplace({ server, initialSetup, onSetupStarted }: { server: string; initialSetup?: PluginSetupContext; onSetupStarted?: (conversation: string) => void }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const queryKey = ['plugin-marketplace', server] as const;
  const items = useQuery({ queryKey, queryFn: pluginApi.marketplace, retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false, retryOnMount: false });
  const [search, setSearch] = useState('');
  const [filter, setFilter] = useState<'all' | 'installed'>('all');
  const [selectedId, setSelectedId] = useState<string | null>(initialSetup?.plugin ?? null);
  const normalized = search.trim().toLocaleLowerCase();
  const visible = (items.data ?? []).filter(item => (filter === 'all' || item.installed) && (!normalized || `${item.manifest.name} ${t(`plugins.${item.manifest.id}.description`)} ${t(`plugins.${item.manifest.id}.category`)}`.toLocaleLowerCase().includes(normalized)));
  const installedCount = items.isError ? '-' : items.data?.filter(item => item.installed).length ?? '-';
  const selected = items.data?.find(item => item.manifest.id === selectedId && item.installed);
  if (selected?.manifest.runtime === 'mcp' && !items.isError) return <ChromeMcpWorkspace plugin={selected} onBack={() => setSelectedId(null)} onRefresh={() => { void items.refetch(); }} onChange={updated => client.setQueryData<MarketplacePlugin[]>(queryKey, current => current?.map(item => item.manifest.id === updated.manifest.id ? updated : item))} />;
  if (selected && !items.isError) return <PluginWorkspace key={server + selected.manifest.id} plugin={selected} initialOptions={initialSetup?.plugin === selected.manifest.id ? initialSetup.options : undefined} onSetupStarted={onSetupStarted} onBack={() => setSelectedId(null)} onRefresh={() => { void items.refetch(); }} onChange={updated => client.setQueryData<MarketplacePlugin[]>(queryKey, current => current?.map(item => item.manifest.id === updated.manifest.id ? updated : item))} />;
  return <div className="plugin-marketplace">
    <p className="plugin-intro">{t('plugins.description')}</p>
    <div className="plugin-toolbar">
      <div className="plugin-filters" role="group" aria-label={t('plugins.filter')}>
        <button type="button" className={filter === 'all' ? 'selected' : ''} aria-pressed={filter === 'all'} onClick={() => setFilter('all')}>{t('plugins.all')}</button>
        <button type="button" className={filter === 'installed' ? 'selected' : ''} aria-pressed={filter === 'installed'} onClick={() => setFilter('installed')}>{t('plugins.installed')}{' '}<span>{installedCount}</span></button>
      </div>
      <label className="plugin-search"><Search size={16} aria-hidden="true" /><input aria-label={t('plugins.search')} placeholder={t('plugins.search')} value={search} onChange={event => setSearch(event.target.value)} /></label>
    </div>
    <p className="plugin-boundary">{t('plugins.boundary')}</p>
    {items.isPending ? <LoadingState /> : items.isError ? <ErrorState compact title={t('plugins.loadFailed')} description={items.error.message} onRetry={() => void items.refetch()} retrying={items.isFetching} /> : !visible.length ? <EmptyState compact title={t(search.trim() ? 'plugins.noResults' : filter === 'installed' ? 'plugins.noneInstalled' : 'plugins.empty')} description={t(search.trim() ? 'plugins.searchHint' : 'plugins.emptyHint')} action={<button type="button" className="ghost" onClick={() => { setSearch(''); setFilter('all'); }}>{t('plugins.browse')}</button>} /> : <div className="plugin-grid">{visible.map(item => <PluginCard key={item.manifest.id} item={item} queryKey={queryKey} busy={items.isFetching} onManage={() => setSelectedId(item.manifest.id)} />)}</div>}
  </div>;
}
