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

export function LoadingState({ label, lines = 3 }: { label?: string; lines?: number }) {
  const { t } = useI18n();
  return <div className="loading-skeleton" role="status" aria-label={label ?? t('common.loading')} aria-busy="true">
    <span className="sr-only">{label ?? t('common.loading')}</span>
    {Array.from({ length: lines }, (_, index) => <i key={index} aria-hidden="true" />)}
  </div>;
}

// Retry only on user input, with the failed operation's variables.
export function MutationError<T>({ mutation, disabled = false, description }: {
  mutation: { error: Error | null; isPending: boolean; variables: T | undefined; mutate: (variables: T) => void };
  disabled?: boolean;
  description?: string;
}) {
  const { t } = useI18n();
  if (!mutation.error) return null;
  return <ErrorState compact title={t('common.operationFailed')} description={description ?? mutation.error.message}
    onRetry={() => { if (!disabled && !mutation.isPending) mutation.mutate(mutation.variables as T); }} retrying={disabled || mutation.isPending} />;
}
