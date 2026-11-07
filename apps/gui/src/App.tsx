import { ClientAuthGate } from './components/ClientAuthGate';
import { ClientAccessManager } from './components/ClientAccessManager';
import { Dialog } from './components/Dialog';
import { accessChange, clientCredentialKey, normalizeCoreUrl } from './lib/clientCredentials';
import { randomId } from './lib/uuid';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Bell, Bot, Check, Link, Plus, Settings2, Folder, Menu, Search, Clock3, Puzzle, Inbox } from 'lucide-react';
import { FormEvent, useEffect, useMemo, useRef, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { api, type PluginSetupContext } from './lib/api';
import { EmptyState, ErrorState, LoadingState } from './components/StateView';
import { ConversationView } from './components/ConversationView';
import { AutomationPanel } from './components/AutomationPanel';
import { AutomationList } from './components/AutomationList';
import { AgentsPanel } from './components/AgentsPanel';
import { PluginMarketplace } from './components/PluginMarketplace';
import { SlackInbox } from './components/SlackInbox';
import { DesktopSettings } from './components/DesktopSettings';
import { NotificationSettings } from './components/NotificationSettings';
import { useSystemNotifications } from './lib/useSystemNotifications';
import { ConversationList } from './components/ConversationList';
import { useI18n, type Locale } from './i18n';
import { isNativeMobile } from './lib/platform';

function ProjectOptions({ recentProjects, knownProjects, search, current, selectedValue, onChoose, ready }: { ready: boolean; recentProjects: string[]; knownProjects: string[]; search: string; current: string; selectedValue?: string; onChoose: (value: string) => void }) {
  const { t } = useI18n();
  const normalized = search.trim().toLowerCase();
  const matches = (value: string) => !normalized || value.toLowerCase().includes(normalized);
  const recent = recentProjects.filter(matches);
  const known = knownProjects.filter((value) => !recent.includes(value) && matches(value));
  return <div className="project-options">
    {recent.length ? <section><h3><Clock3 size={13} />{t('project.recent')}</h3>{recent.map((value) => <button type="button" className={`project-option${value === (selectedValue ?? current) ? ' selected' : ''}`} key={`recent-${value}`} onClick={() => onChoose(value)}><Folder size={15} /><span>{value}</span></button>)}</section> : null}
    {known.length ? <section><h3><Folder size={13} />{t('project.known')}</h3>{known.map((value) => <button type="button" className={`project-option${value === selectedValue ? ' selected' : ''}`} key={`known-${value}`} onClick={() => onChoose(value)}><Folder size={15} /><span>{value}</span></button>)}</section> : null}
    {ready && !recent.length && !known.length && normalized && !/^(\/|~)/.test(normalized) ? <EmptyState compact title={t('project.noMatch')} description={t('project.noSearchResults')} /> : null}
  </div>;
}

export default function App() {
  const { locale, setLocale, t } = useI18n();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const location = useLocation();
  const route = useMemo(() => {
    const parts = location.pathname.split('/').filter(Boolean);
    if (parts[0] === 'slack') return { view: 'slack' as const };
    if (parts[0] === 'chat' && parts[1]) return { view: 'chat' as const, conversationId: decodeURIComponent(parts.slice(1).join('/')) };
    return { view: 'chat' as const };
  }, [location.pathname]);
  const view = route.view;
  const [project, setProject] = useState('');
  const [conversation, setConversation] = useState(() => {
    const id = route.conversationId ?? localStorage.getItem('omnistream.conversation') ?? randomId();
    localStorage.setItem('omnistream.conversation', id);
    return id;
  });
  const [serverUrl, setServerUrl] = useState(api.baseUrl);
  const [connectedUrl, setConnectedUrl] = useState(api.baseUrl);
  const [automationId, setAutomationId] = useState<number | 'new' | undefined>();
  const [dialog, setDialog] = useState<'connection' | 'automation' | 'project' | 'plugins' | 'agents' | 'notifications' | null>(() =>
    isNativeMobile() && !localStorage.getItem('omnistream.url') && !import.meta.env.VITE_OMNISTREAM_URL ? 'connection' : null
  );
  const [projectSearch, setProjectSearch] = useState('');
  const [pluginSetup, setPluginSetup] = useState<PluginSetupContext>();
  const [projectSelection, setProjectSelection] = useState(0);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const projectInputRef = useRef<HTMLInputElement>(null);
  const sidebarScroll = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (route.conversationId && route.conversationId !== conversation) setConversation(route.conversationId);
    if (!route.conversationId && route.view === 'chat') navigate(`/chat/${encodeURIComponent(conversation)}`, { replace: true });
  }, [conversation, navigate, route.conversationId, route.view]);

  useEffect(() => {
    const onShortcut = (event: globalThis.KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === 'p') {
        event.preventDefault();
        setProjectSearch('');
        setDialog('project');
      }
    };
    window.addEventListener('keydown', onShortcut);
    return () => window.removeEventListener('keydown', onShortcut);
  }, []);

  const rememberProject = (value: string) => {
    const normalized = value.trim();
    if (!normalized) return;
    const projects = [normalized, ...recentProjects.filter((item) => item !== normalized)].slice(0, 12);
    localStorage.setItem('omnistream.projects', JSON.stringify(projects));
  };
  const chooseProject = (value: string) => {
    setProject(value);
    rememberProject(value);
    setDialog(null);
  };
  const health = useQuery({
    queryKey: ['health', connectedUrl],
    queryFn: api.health,
    retry: false,
    refetchInterval: query => query.state.status === 'error' ? false : 15_000,
  });
  const accessReady = health.isSuccess && health.data.access?.authorized !== false;
  const needsAccess = health.isSuccess && health.data.access?.authorized === false;
  useEffect(() => {
    let normalized: string;
    try { normalized = normalizeCoreUrl(connectedUrl); } catch { return; }
    const changed = (event: Event) => { if ((event as CustomEvent<string>).detail === normalized) void health.refetch(); };
    const storage = (event: StorageEvent) => { if (!event.key || event.key === clientCredentialKey(normalized)) void health.refetch(); };
    window.addEventListener(accessChange, changed); window.addEventListener('storage', storage);
    return () => { window.removeEventListener(accessChange, changed); window.removeEventListener('storage', storage); };
  }, [connectedUrl, health.refetch]);
  const [pathQuery, setPathQuery] = useState('');
  useEffect(() => {
    const timer = window.setTimeout(() => setPathQuery(projectSearch.trim()), 180);
    return () => window.clearTimeout(timer);
  }, [projectSearch]);
  const isPathSearch = /^(\/|~)/.test(projectSearch.trim());
  const pathSuggestions = useQuery({
    queryKey: ['project-paths', connectedUrl, pathQuery],
    queryFn: () => api.completeProjectPath(pathQuery),
    enabled: accessReady && dialog === 'project' && /^(\/|~)/.test(pathQuery),
    retry: false,
    staleTime: 5000,
  });
  const notifications = useSystemNotifications(connectedUrl, accessReady, path => { setDialog(null); navigate(path); });
  const recentProjects = useMemo(() => {
    const saved = (() => {
      try { return JSON.parse(localStorage.getItem('omnistream.projects') ?? '[]') as unknown; } catch { return []; }
    })();
    return [...new Set(Array.isArray(saved) ? saved : [])].filter((item): item is string => typeof item === 'string').slice(0, 12);
  }, [project]);
  const registeredProjects = useQuery({ queryKey: ['projects', connectedUrl], queryFn: api.projects, enabled: accessReady });
  const knownProjects = useMemo(() => [...new Set((registeredProjects.data ?? []).map(item => item.path).filter(Boolean))], [registeredProjects.data]);
  const normalizedProjectSearch = projectSearch.trim().toLowerCase();
  const matchingRecentProjects = recentProjects.filter((value) => !normalizedProjectSearch || value.toLowerCase().includes(normalizedProjectSearch));
  const matchingKnownProjects = knownProjects.filter((value) => !matchingRecentProjects.includes(value) && (!normalizedProjectSearch || value.toLowerCase().includes(normalizedProjectSearch)));
  const projectCandidates = isPathSearch ? (pathSuggestions.data ?? []) : [...matchingRecentProjects, ...matchingKnownProjects];
  const selectedProjectCandidate = projectCandidates[projectSelection];
  useEffect(() => {
    setProjectSelection(0);
  }, [projectSearch, pathSuggestions.data]);
  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ['approvals'] });
    void queryClient.invalidateQueries({ queryKey: ['conversation', connectedUrl, conversation] });
    void queryClient.invalidateQueries({ queryKey: ['conversations', connectedUrl] });
  };
  const reconnect = (event: FormEvent) => {
    event.preventDefault();
    api.setBaseUrl(serverUrl);
    setConnectedUrl(serverUrl.trim().replace(/\/$/, ''));
    if (api.baseUrl === connectedUrl) void health.refetch();
    setDialog(null);
    refresh();
  };
  const retryConnection = () => {
    void health.refetch();
  };
  const selectConversation = (id: string) => {
    setConversation(id);
    localStorage.setItem('omnistream.conversation', id);
    navigate(`/chat/${encodeURIComponent(id)}`);
    setSidebarOpen(false);
  };
  const conversationAction = async (id: string, action: 'rename' | 'archive' | 'delete', title?: string) => {
    if (action === 'rename') {
      const settings = await api.renameConversation(id, title ?? '');
      queryClient.setQueryData(['conversation-settings', connectedUrl, id], settings);
      await queryClient.invalidateQueries({ queryKey: ['conversation-detail', connectedUrl, id] });
      await queryClient.invalidateQueries({ queryKey: ['conversations', connectedUrl] });
      return;
    }
    if (action === 'delete') {
      await api.deleteConversation(id);
      queryClient.removeQueries({ queryKey: ['conversation', connectedUrl, id] });
      queryClient.removeQueries({ queryKey: ['conversation-detail', connectedUrl, id] });
      queryClient.removeQueries({ queryKey: ['conversation-settings', connectedUrl, id] });
      if (id === conversation) newConversation();
    } else await api.archiveConversation(id);
    await queryClient.invalidateQueries({ queryKey: ['conversations', connectedUrl] });
  };
  const newConversation = () => selectConversation(randomId());
  return (
    <main className="shell">
      <header className="topbar">
        <div className="brand"><button type="button" className="icon-button sidebar-toggle" aria-label={t('nav.toggleSidebar')} title={t('nav.toggleSidebar')} aria-expanded={sidebarOpen} aria-controls="workspace-sidebar" onClick={() => setSidebarOpen(!sidebarOpen)}><Menu size={19} /></button><img className="brand-logo" src="/omnistream-logo.svg" alt="OmniStream" /></div>
        <div className="header-tools">
          <span className={`connection-state ${health.isSuccess ? needsAccess ? 'checking' : 'online' : health.isError ? 'offline' : 'checking'}`}>
            <span className="connection-dot" />
            {health.isSuccess ? needsAccess ? t('access.required') : t('status.connected') : health.isError ? t('status.coreOffline') : t('status.connecting')}
          </span>
          <label className="language-picker"><span className="sr-only">{t('language.label')}</span><select aria-label={t('language.label')} value={locale} onChange={event => setLocale(event.target.value as Locale)}><option value="zh-CN">{t('language.zh')}</option><option value="en-US">{t('language.en')}</option></select></label>
          <ClientAccessManager key={connectedUrl} server={connectedUrl} enabled={accessReady && !!health.data?.access} />
          <button type="button" className={`icon-button notification-trigger${notifications.feedError || notifications.readError || notifications.permissionError || notifications.remote.error || ['paused', 'disabled', 'unregistered'].includes(notifications.remote.device?.state ?? '') ? ' has-error' : ''}`} title={t(notifications.feedError ? 'notifications.paused' : 'notifications.settings')} aria-label={t('notifications.settings')} disabled={!accessReady} onClick={() => setDialog('notifications')}><Bell size={18} /></button>
          <button type="button" className="icon-button" title={t('nav.connectionSettings')} aria-label={t('nav.connectionSettings')} onClick={() => setDialog('connection')}><Settings2 size={18} /></button>
        </div>
      </header>
      {accessReady && dialog === 'notifications' && <Dialog title={t('notifications.settings')} onClose={() => setDialog(null)}><NotificationSettings state={notifications} /></Dialog>}
      {dialog === 'connection' ? <Dialog title={t('dialog.connectionSettings')} onClose={() => setDialog(null)}>
        <DesktopSettings />
        <form className="dialog-form" onSubmit={reconnect}>
          {isNativeMobile() && <p className="muted">{t('dialog.mobileCoreHint')}</p>}
          <label>{t('dialog.coreAddress')}<input autoFocus required type="url" aria-label={t('dialog.coreAddress')} value={serverUrl} onChange={(event) => setServerUrl(event.target.value)} placeholder="http://127.0.0.1:4317" /></label>
          <div className="dialog-actions"><button type="button" className="ghost" onClick={() => setDialog(null)}>{t('common.cancel')}</button><button type="submit" className="primary"><Link size={14} />{t('common.connect')}</button></div>
        </form>
      </Dialog> : null}
      {accessReady && dialog === 'automation' ? <Dialog title={t(automationId === undefined ? 'automation.title' : automationId === 'new' ? 'automation.create' : 'automation.details')} className="automation-dialog" onClose={() => setDialog(null)}>
        {automationId === undefined ? <AutomationList server={connectedUrl} onOpen={setAutomationId} onCreate={() => setAutomationId('new')} /> : <>
          <button type="button" className="ghost automation-back" onClick={() => setAutomationId(undefined)}>{t('automation.backToList')}</button>
          <AutomationPanel key={automationId} selectedId={automationId === 'new' ? undefined : automationId} onCreated={setAutomationId} server={connectedUrl} conversation={conversation} onOpenConversation={id => { selectConversation(id); setDialog(null); }} />
        </>}
      </Dialog> : null}
      {accessReady && dialog === 'plugins' ? <Dialog title={t('plugins.title')} className="plugin-marketplace-dialog" onClose={() => setDialog(null)}>
        <PluginMarketplace key={connectedUrl} server={connectedUrl} initialSetup={pluginSetup} onSetupStarted={id => { selectConversation(id); setDialog(null); void queryClient.invalidateQueries({ queryKey: ['conversations', connectedUrl] }); }} />
      </Dialog> : null}
      {accessReady && dialog === 'agents' ? <Dialog title={t('agents.title')} className="automation-dialog" onClose={() => setDialog(null)}><AgentsPanel server={connectedUrl} /></Dialog> : null}
      {accessReady && dialog === 'project' ? <Dialog title={t('dialog.selectProject')} className="project-dialog" onClose={() => setDialog(null)}>
        <div className="project-picker">
          <label className="project-search"><Search size={16} /><input autoFocus value={projectSearch} onChange={(event) => setProjectSearch(event.target.value)} onKeyDown={(event) => { if (event.nativeEvent.isComposing) return; if (event.key === 'ArrowDown' && projectCandidates.length) { event.preventDefault(); setProjectSelection((index) => Math.min(index + 1, projectCandidates.length - 1)); } if (event.key === 'ArrowUp' && projectCandidates.length) { event.preventDefault(); setProjectSelection((index) => Math.max(index - 1, 0)); } if (event.key === 'Enter' && (selectedProjectCandidate || projectSearch.trim())) { event.preventDefault(); chooseProject(selectedProjectCandidate ?? projectSearch.trim()); } }} placeholder={t('dialog.searchProject')} />{isPathSearch && pathSuggestions.isFetching ? <span className="project-loading" role="status" aria-label={t('common.loading')} /> : null}</label>
          {!isPathSearch && registeredProjects.isPending && <LoadingState />}
          {!isPathSearch && registeredProjects.isError && <ErrorState compact title={t('project.pathLoadFailed')} description={registeredProjects.error.message} onRetry={() => void registeredProjects.refetch()} retrying={registeredProjects.isFetching} />}
          <ProjectOptions ready={registeredProjects.isSuccess} recentProjects={recentProjects} knownProjects={knownProjects} search={projectSearch} current={project} selectedValue={selectedProjectCandidate} onChoose={chooseProject} />
          {isPathSearch ? <section className="project-path-results" aria-label={t('dialog.pathSuggestions')}>
            {isPathSearch && pathSuggestions.isFetching && !pathSuggestions.data && <LoadingState />}
            {pathSuggestions.isError ? <ErrorState compact title={t('project.pathLoadFailed')} description={pathSuggestions.error.message} onRetry={() => { void pathSuggestions.refetch(); }} retrying={pathSuggestions.isFetching} /> : <>
              {(pathSuggestions.data ?? []).map((path) => <div className="project-path-row" key={path}>
                <button type="button" className={`project-option${path === selectedProjectCandidate ? ' selected' : ''}`} title={path} onMouseEnter={() => setProjectSelection(projectCandidates.indexOf(path))} onClick={() => chooseProject(path)}><Folder size={15} /><span>{path}</span></button>
              </div>)}
              {pathSuggestions.isSuccess && !pathSuggestions.isFetching && !pathSuggestions.data?.length && pathQuery === projectSearch.trim() ? <EmptyState compact title={t('project.noFolders')} description={t('project.noPathResults')} /> : null}
            </>}
          </section> : null}
          <div className="dialog-actions"><button type="button" className="ghost" onClick={() => setDialog(null)}>{t('common.cancel')}</button><button className="primary" type="button" onClick={() => chooseProject(selectedProjectCandidate ?? projectSearch.trim())} disabled={!projectSearch.trim()}>{t('common.confirm')}</button></div>
        </div>
      </Dialog> : null}
      {health.isError ? (
        <section className="connection-failure">
          <ErrorState title={t('connection.unavailable')} description={t('connection.description')} onRetry={retryConnection} retrying={health.isFetching} retryLabel={t('connection.retry')}
            action={<button className="ghost" type="button" onClick={() => setDialog('connection')}><Settings2 size={14} />{t('connection.checkAddress')}</button>} />
          <p className="failure-address">{connectedUrl}</p>
        </section>
      ) : needsAccess ? <ClientAuthGate key={connectedUrl} server={connectedUrl} onAuthorized={health.refetch} onChangeServer={() => setDialog('connection')} /> : health.isPending ? <section className="connection-failure"><LoadingState /></section> : <section className="content">
        <aside id="workspace-sidebar" className={`sidebar${sidebarOpen ? ' is-open' : ''}`}>
          <button className="new-conversation primary" onClick={newConversation}><Plus size={16} />{t('nav.newConversation')}</button>
          <nav className="sidebar-nav">
            <button type="button" className="new-conversation ghost" onClick={() => { setPluginSetup(undefined); setDialog('plugins'); setSidebarOpen(false); }}><Puzzle size={16} />{t('plugins.title')}</button>
            <button type="button" className="new-conversation ghost" onClick={() => { setDialog('agents'); setSidebarOpen(false); }}><Bot size={16} />{t('agents.title')}</button>
            <button type="button" className="new-conversation ghost" aria-current={view === 'slack' ? 'page' : undefined} onClick={() => { navigate('/slack'); setSidebarOpen(false); }}><Inbox size={16} />{t('slack.inbox')}</button>
          </nav>
          <div className="sidebar-scroll" ref={sidebarScroll}>
          <ConversationList key={connectedUrl} server={connectedUrl} conversation={conversation} active={view === 'chat'} enabled={accessReady} scrollContainer={sidebarScroll} onSelect={selectConversation} onAction={conversationAction} />
          <button type="button" className="new-conversation ghost" onClick={() => { setAutomationId(undefined); setDialog('automation'); }}><Clock3 size={16} />{t('automation.title')}</button>
          </div>
          <div className="sidebar-footer"><button type="button" className="project-trigger" onClick={() => { setProjectSearch(''); setDialog('project'); }} title={project || t('nav.selectProject')}><Folder size={14} /><span>{project || t('nav.selectProject')}</span><kbd>⌘/Ctrl⇧P</kbd></button><input ref={projectInputRef} id="project-path" value={project} onChange={(event) => setProject(event.target.value)} title={project} placeholder={t('nav.projectPath')} /></div>
        </aside>
        <section className="workspace">
          {view === 'slack' && <SlackInbox onRead={health.isSuccess ? notifications.read : undefined} key={connectedUrl} server={connectedUrl} requestedChannel={new URLSearchParams(location.search).get('channel')} onConfigure={() => { setPluginSetup(undefined); setDialog('plugins'); }} />}
          {view === 'chat' && <ConversationView onRead={health.isSuccess ? notifications.read : undefined} onOpenAutomation={id => { setAutomationId(id); setDialog('automation'); }} key={connectedUrl + conversation} onConfigurePlugin={context => { setPluginSetup(context); setDialog('plugins'); }} id={conversation} server={connectedUrl} project={project} projects={[...new Set([...recentProjects, ...knownProjects])]} onNew={newConversation} />}
        </section>
      </section>}
    </main>
  );
}
