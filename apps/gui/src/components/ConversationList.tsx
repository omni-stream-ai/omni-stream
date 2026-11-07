import { useInfiniteQuery, useQuery } from '@tanstack/react-query';
import { useEffect, useRef, type RefObject } from 'react';
import { api, type ConversationCursor } from '../lib/api';
import { useI18n } from '../i18n';
import { ConversationItem } from './ConversationItem';
import { ErrorState, LoadingState } from './StateView';

export function ConversationList({ server, conversation, active, enabled, scrollContainer, onSelect, onAction }: {
  server: string;
  conversation: string;
  active: boolean;
  enabled: boolean;
  scrollContainer: RefObject<HTMLDivElement | null>;
  onSelect: (id: string) => void;
  onAction: (id: string, action: 'rename' | 'archive' | 'delete', title?: string) => Promise<void>;
}) {
  const { t } = useI18n();
  const sentinel = useRef<HTMLDivElement>(null);
  const conversations = useInfiniteQuery({
    queryKey: ['conversations', server],
    queryFn: ({ pageParam }) => api.conversations(pageParam),
    initialPageParam: null as ConversationCursor | null,
    getNextPageParam: page => page.next_cursor,
    enabled,
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    retryOnMount: false,
    refetchInterval: query => query.state.status === 'error' ? false : 15_000,
  });
  const items = [...new Map((conversations.data?.pages ?? []).flatMap(page => page.items).map(item => [item.id, item])).values()];
  const listed = items.some(item => item.id === conversation);
  const current = useQuery({
    queryKey: ['conversation-detail', server, conversation],
    queryFn: () => api.conversationDetail(conversation),
    enabled: enabled && !listed,
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
  const draft = !listed && current.data?.exists === false;
  const { hasNextPage, isFetching, isError, fetchNextPage } = conversations;
  useEffect(() => {
    if (scrollContainer.current) scrollContainer.current.scrollTop = 0;
  }, [server, scrollContainer]);
  useEffect(() => {
    if (!enabled || !hasNextPage || isFetching || isError || !sentinel.current || !scrollContainer.current) return;
    const observer = new IntersectionObserver(entries => {
      if (entries.some(entry => entry.isIntersecting)) void fetchNextPage({ cancelRefetch: false });
    }, { root: scrollContainer.current, rootMargin: '0px 0px 120px 0px' });
    observer.observe(sentinel.current);
    return () => observer.disconnect();
  }, [enabled, hasNextPage, isFetching, isError, fetchNextPage, scrollContainer]);
  return <>
    <div className="section-heading sidebar-heading"><h2>{t('nav.conversations')}</h2><span className="count">{conversations.data ? conversations.data.pages[0].total + (draft ? 1 : 0) : '-'}</span></div>
    <div className="conversation-list">
      {conversations.isPending && <LoadingState />}
      {draft && <div className={`conversation-row${active ? ' active' : ''}`}><button type="button" className="conversation-item" aria-current={active ? 'page' : undefined} onClick={() => onSelect(conversation)}><span>{t('conversation.new')}</span></button></div>}
      {items.map(item => <ConversationItem key={item.id} title={item.title || t('conversation.new')} active={active && item.id === conversation} onSelect={() => onSelect(item.id)} onAction={(action, title) => onAction(item.id, action, title)} />)}
      <div ref={sentinel} className="conversation-list-sentinel" aria-hidden="true" />
      {conversations.isFetchingNextPage && <LoadingState label={t('conversation.loadingMore')} lines={2} />}
      {conversations.isError && <ErrorState compact title={t('conversation.loadFailed')} description={t('conversation.listUnavailable')} onRetry={() => { void (conversations.isFetchNextPageError ? conversations.fetchNextPage({ cancelRefetch: false }) : conversations.refetch()); }} retrying={conversations.isFetching} />}
    </div>
  </>;
}
