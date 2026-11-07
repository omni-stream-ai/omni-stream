import { X } from 'lucide-react';
import { useEffect, useRef, type ReactNode } from 'react';
import { useI18n } from '../i18n';

export function Dialog({ title, onClose, children, className }: { title: string; onClose: () => void; children: ReactNode; className?: string }) {
  const { t } = useI18n();
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    ref.current?.showModal();
    ref.current?.querySelector<HTMLInputElement>('input')?.focus();
  }, []);
  return <dialog ref={ref} className={'dialog' + (className ? ' ' + className : '')} aria-label={title} onCancel={onClose} onClick={event => { if (event.target === event.currentTarget) onClose(); }}>
    <div className="dialog-body">
      <div className="dialog-heading"><h2>{title}</h2><button type="button" className="icon-button" aria-label={t('common.close')} title={t('common.close')} onClick={onClose}><X size={18} /></button></div>
      {children}
    </div>
  </dialog>;
}
