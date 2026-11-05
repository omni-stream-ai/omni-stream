import { useEffect, useRef, useState } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { ExternalLink, LoaderCircle, RefreshCw, Reply, Send, Settings2, X } from 'lucide-react';
import { slackApi, type PluginConnection, type SlackMessage } from '../lib/api';
import { useI18n } from '../i18n';
import { EmptyState, ErrorState } from './StateView';
import './SlackInbox.css';

export function SlackInbox({ server, onConfigure }: { server: string; onConfigure: () => void }) {
  const { t } = useI18n();
  const state = useQuery({ queryKey: ['slack-inbox', server], queryFn: slackApi.inbox, retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false, refetchInterval: query => query.state.status === 'error' ? false : 3000 });
  const reconnect = useMutation({ mutationFn: slackApi.reconnect, retry: false, onSuccess: () => void state.refetch() });
  const [selection, setSelection] = useState('');
  const channel = state.data?.conversations.find(item => item.channel === selection) ?? state.data?.conversations[0];
  useEffect(() => { if (channel && channel.channel !== selection) setSelection(channel.channel); }, [channel?.channel, selection]);
  const connection = state.data?.connection;
  return <section className="slack-inbox" aria-label={t('slack.inbox')}>
    <header className="slack-inbox-heading"><div><h1>{t('slack.inbox')}</h1>{connection?.account && <p>{t('plugins.account', { account: connection.account })}</p>}</div><div className="slack-inbox-tools"><button type="button" className="ghost" onClick={onConfigure}><Settings2 size={14} />{t('plugins.manage')}</button><button type="button" className="ghost" disabled={state.isFetching} onClick={() => void state.refetch()}><RefreshCw size={14} />{t('slack.refresh')}</button></div></header>
    {state.isPending && <p role="status">{t('common.loading')}</p>}
    {state.isError && <ErrorState compact title={t('slack.loadFailed')} description={state.error.message} retrying={state.isFetching} onRetry={() => void state.refetch()} />}
    {state.data && <>
      <div className="slack-listener-status" role="status"><span className={'connection-dot ' + state.data.listener.state} /><strong>{t('slack.state.' + state.data.listener.state)}</strong>{connection?.listen && <span>{t('slack.scope.' + (connection.listen_scope ?? 'related'))}</span>}{connection?.listen && <button type="button" className="ghost" disabled={reconnect.isPending} onClick={() => reconnect.mutate()}>{t('slack.reconnect')}</button>}</div>
      {(state.data.listener.error || reconnect.error) && <p role="alert" className="error">{reconnect.error?.message ?? state.data.listener.error}</p>}
      {!connection?.listen && <p className="muted slack-inbox-note">{t('slack.enableHint')}</p>}
      {!channel ? <EmptyState title={t('slack.empty')} description={t('slack.emptyHint')} /> : <div className="slack-inbox-layout">
        <nav className="slack-channels" aria-label={t('slack.conversations')}>{state.data.conversations.map(item => <button type="button" className={channel.channel === item.channel ? 'selected' : ''} key={item.channel} onClick={() => setSelection(item.channel)}><strong>{t('slack.channelType.' + (['im', 'mpim', 'group'].includes(item.channel_type) ? item.channel_type : 'channel'))} · {item.channel}</strong><span>{item.text || t('slack.attachment')}</span></button>)}</nav>
        <SlackChannel key={[server, connection?.team_id, connection?.user_id, channel.channel].join(':')} server={server} channel={channel.channel} connection={connection!} onSent={() => void state.refetch()} />
      </div>}
    </>}
  </section>;
}

function SlackChannel({ server, channel, connection, onSent }: { server: string; channel: string; connection: PluginConnection; onSent: () => void }) {
  const { t, locale } = useI18n();
  const messages = useQuery({ queryKey: ['slack-messages', server, connection.team_id, connection.user_id, channel], queryFn: () => slackApi.messages(channel), retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false, refetchInterval: query => query.state.status === 'error' ? false : 3000 });
  const [older, setOlder] = useState<SlackMessage[]>([]);
  const [before, setBefore] = useState<string | null | undefined>(undefined);
  const [text, setText] = useState('');
  const [reply, setReply] = useState<SlackMessage | null>(null);
  const scroll = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const page = useMutation({ mutationFn: (cursor: string) => slackApi.messages(channel, cursor), retry: false, onSuccess: result => { setOlder(current => [...result.messages, ...current]); setBefore(result.next_before); } });
  const send = useMutation({ mutationFn: () => slackApi.send({ team_id: connection.team_id!, user_id: connection.user_id!, channel, text: text.trim(), ...(reply ? { thread_ts: reply.thread_ts ?? reply.ts } : {}) }), retry: false,
    onSuccess: () => { setText(''); setReply(null); stick.current = true; void messages.refetch(); onSent(); },
  });
  const available = connection.enabled && connection.allow_writes && connection.auth_mode === 'slack_user' && !!connection.team_id && !!connection.user_id;
  const rows = [...new Map([...older, ...(messages.data?.messages ?? [])].map(item => [item.ts, item])).values()].sort((a, b) => Number(a.ts) - Number(b.ts));
  const cursor = before === undefined ? messages.data?.next_before : before;
  useEffect(() => { if (stick.current && scroll.current) scroll.current.scrollTop = scroll.current.scrollHeight; }, [messages.data, channel]);
  return <section className="slack-channel" aria-label={t('slack.messages')}>
    <header><strong>{channel}</strong><a href={'https://app.slack.com/client/' + encodeURIComponent(connection.team_id ?? '') + '/' + encodeURIComponent(channel)} target="_blank" rel="noreferrer">{t('slack.openSlack')}<ExternalLink size={12} /></a></header>
    <div className="slack-messages" ref={scroll} onScroll={() => { const element = scroll.current; if (element) stick.current = element.scrollHeight - element.scrollTop - element.clientHeight < 80; }}>
      {messages.isPending && <p role="status">{t('common.loading')}</p>}
      {messages.isError && <ErrorState compact title={t('slack.loadFailed')} description={messages.error.message} onRetry={() => void messages.refetch()} retrying={messages.isFetching} />}
      {cursor && <button type="button" className="ghost" disabled={page.isPending} onClick={() => { stick.current = false; page.mutate(cursor); }}>{t('slack.loadEarlier')}</button>}
      {page.error && <p className="error" role="alert">{page.error.message}</p>}
      {rows.map(message => <article key={message.ts} className={'slack-message' + (message.own ? ' own' : '')}>
        <header><strong>{message.own ? t('slack.you') : message.user || t('slack.unknownSender')}</strong><time dateTime={new Date(Number(message.ts) * 1000).toISOString()}>{new Date(Number(message.ts) * 1000).toLocaleString(locale)}</time>{message.edited && <span>{t('slack.edited')}</span>}</header>
        <p>{message.deleted ? t('slack.deleted') : message.text || t('slack.attachment')}</p>
        {message.thread_ts && <span className="muted">{t('slack.thread', { ts: message.thread_ts })}</span>}
        {!message.deleted && <button type="button" className="ghost" disabled={!available || send.isPending} onClick={() => { setReply(message); send.reset(); }}><Reply size={12} />{t('slack.replyThread')}</button>}
      </article>)}
    </div>
    <form className="slack-send" onSubmit={event => { event.preventDefault(); if (available && text.trim() && !send.isPending) send.mutate(); }}>
      {reply && <div className="slack-reply-target"><span>{t('slack.replyingTo', { user: reply.user })}: {reply.text.slice(0,120)}</span><button type="button" className="icon-button" aria-label={t('slack.cancelReply')} disabled={send.isPending} onClick={() => setReply(null)}><X size={14} /></button></div>}
      <label className="sr-only" htmlFor="slack-message-text">{t('slack.message')}</label><textarea id="slack-message-text" value={text} maxLength={4000} disabled={!available || send.isPending} placeholder={t('slack.messagePlaceholder')} rows={3} onChange={event => { setText(event.target.value); send.reset(); }} />
      {!available && <p className="muted">{t('slack.sendDisabled')}</p>}
      <div className="slack-send-footer"><p className="muted">{t('slack.sendPolicy')}</p><button type="submit" className="primary" disabled={!available || !text.trim() || send.isPending}>{send.isPending ? <LoaderCircle className="spin" size={14} /> : <Send size={14} />}{t('slack.send')}</button></div>
      {send.error && <p className="error" role="alert">{send.error.message} {t('plugins.checkBeforeRetry')}</p>}
      {send.isSuccess && <p role="status" className="plugin-success">{send.data.sync_warning ?? t('slack.sent')}</p>}
    </form>
  </section>;
}
