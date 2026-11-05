import { RotateCw } from 'lucide-react';
import type { ReactNode } from 'react';
import './StateView.css';
import { useI18n } from '../i18n';

type StateProps = {
  title: string;
  description: string;
  compact?: boolean;
  action?: ReactNode;
};

function StateView({ title, description, compact, action, failed = false }: StateProps & { failed?: boolean }) {
  return <div className={`state-view${compact ? ' state-view--compact' : ''}`} role={failed ? 'alert' : undefined}>
    <img className="state-view__illustration" src={`/illustrations/${failed ? 'error' : 'empty'}.svg`} width="180" height="128" alt="" aria-hidden="true" />
    <h2>{title}</h2>
    <p>{description}</p>
    {action ? <div className="state-view__actions">{action}</div> : null}
  </div>;
}

export function EmptyState(props: StateProps) {
  return <StateView {...props} />;
}

export function ErrorState({ onRetry, retrying = false, retryLabel = '重试', action, ...props }: StateProps & {
  onRetry: () => void;
  retrying?: boolean;
  retryLabel?: string;
}) {
  const { t } = useI18n();
  return <StateView {...props} failed action={<>
    <button type="button" className="ghost" onClick={onRetry} disabled={retrying} aria-busy={retrying}>
      <RotateCw size={14} className={retrying ? 'spin' : undefined} />
      {retrying ? t('common.loading') : retryLabel === '重试' ? t('common.retry') : retryLabel}
    </button>
    {action}
  </>} />;
}
