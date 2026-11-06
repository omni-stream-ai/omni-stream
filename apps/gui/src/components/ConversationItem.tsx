import { ErrorState } from './StateView';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Archive, Check, Ellipsis, Pencil, Trash2, X } from 'lucide-react';
import { useI18n } from '../i18n';

export function ConversationItem({ title, active, onSelect, onAction }: {
  title: string;
  active: boolean;
  onSelect: () => void;
  onAction: (action: 'rename' | 'archive' | 'delete', title?: string) => Promise<void>;
}) {
  const { t } = useI18n();
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<{ top: number; left: number } | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState(title);
  const [busy, setBusy] = useState(false);
  const [failedAction, setFailedAction] = useState<'rename' | 'archive' | 'delete'>('rename');
  const [error, setError] = useState('');
  useEffect(() => {
    if (!position) return;
    menu.current?.querySelector<HTMLButtonElement>('button')?.focus({ preventScroll: true });
    const close = () => setPosition(null);
    const onScroll = (event: Event) => {
      if (event.target instanceof Node && event.target.contains(trigger.current)) close();
    };
    const outside = (event: PointerEvent) => {
      if (!menu.current?.contains(event.target as Node) && !trigger.current?.contains(event.target as Node)) close();
    };
    window.addEventListener('pointerdown', outside);
    window.addEventListener('resize', close);
    window.addEventListener('scroll', onScroll, true);
    return () => {
      window.removeEventListener('pointerdown', outside);
      window.removeEventListener('resize', close);
      window.removeEventListener('scroll', onScroll, true);
    };
  }, [position]);
  const act = async (action: 'rename' | 'archive' | 'delete') => {
    setPosition(null); setBusy(true); setError('');
    try { await onAction(action, name.trim()); setRenaming(false); }
    catch (cause) { setFailedAction(action); setError(cause instanceof Error ? cause.message : t('conversation.operationFailed')); }
    finally { setBusy(false); }
  };
  return <div className="conversation-list-entry">
    <div className={`conversation-row${active ? ' active' : ''}${position ? ' menu-open' : ''}`}>
      {renaming ? <form className="conversation-rename" onSubmit={event => { event.preventDefault(); void act('rename'); }}>
        <input autoFocus aria-label={t('conversation.name')} value={name} disabled={busy} onFocus={event => event.target.select()} onChange={event => setName(event.target.value)} onKeyDown={event => { if (event.key === 'Escape') setRenaming(false); }} />
        <button className="icon-button" title={t('conversation.saveName')} aria-label={t('conversation.saveName')} disabled={busy || !name.trim()}><Check size={15} /></button>
        <button type="button" className="icon-button" title={t('conversation.cancelRename')} aria-label={t('conversation.cancelRename')} disabled={busy} onClick={() => setRenaming(false)}><X size={15} /></button>
      </form> : <>
        <button type="button" className="conversation-item" title={title} aria-current={active ? 'page' : undefined} onClick={onSelect}><span>{title}</span></button>
        <button ref={trigger} type="button" className="icon-button conversation-more" title={t('conversation.moreActions')} aria-label={t('conversation.actionsFor', { title })} aria-haspopup="menu" aria-expanded={!!position} disabled={busy} onClick={() => {
          if (position) { setPosition(null); return; }
          const rect = trigger.current!.getBoundingClientRect();
          setPosition({ left: Math.max(8, Math.min(rect.right - 156, window.innerWidth - 164)), top: Math.max(8, Math.min(rect.bottom + 4, window.innerHeight - 138)) });
        }}><Ellipsis size={17} /></button>
      </>}
    </div>
    {error && <ErrorState compact title={t('conversation.operationFailed')} description={error} onRetry={() => void act(failedAction)} retrying={busy} />}
    {position && createPortal(<div ref={menu} className="conversation-menu" style={position} role="menu" aria-label={t('conversation.moreActions')} onKeyDown={event => {
      if (event.key === 'Escape' || event.key === 'Tab') { setPosition(null); if (event.key === 'Escape') { event.preventDefault(); trigger.current?.focus(); } }
      if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
        event.preventDefault();
        const items = Array.from(menu.current!.querySelectorAll<HTMLButtonElement>('button'));
        const index = items.indexOf(document.activeElement as HTMLButtonElement);
        items[event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length].focus();
      }
    }}>
      <button role="menuitem" onClick={() => { setPosition(null); setName(title); setRenaming(true); }}><Pencil size={15} />{t('conversation.rename')}</button>
      <button role="menuitem" onClick={() => void act('archive')}><Archive size={15} />{t('conversation.archive')}</button>
      <button role="menuitem" className="conversation-delete" onClick={() => void act('delete')}><Trash2 size={15} />{t('conversation.delete')}</button>
    </div>, document.body)}
  </div>;
}
