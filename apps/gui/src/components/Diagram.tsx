import { ErrorState, LoadingState } from './StateView';
import { useEffect, useRef, useState } from 'react';
import { useI18n } from '../i18n';

export function Diagram({ source }: { source: string }) {
  const { t } = useI18n();
  const container = useRef<HTMLDivElement>(null);
  const [attempt, setAttempt] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    setError(''); setLoading(true);
    if (container.current) container.current.innerHTML = '';
    void import('mermaid').then(async ({ default: mermaid }) => {
      mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', theme: 'neutral', suppressErrorRendering: true });
      const { svg } = await mermaid.render(`diagram-${crypto.randomUUID()}`, source);
      if (active && container.current) container.current.innerHTML = svg;
    }).catch(() => { if (active) setError(t('diagram.invalid')); }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [source, t, attempt]);
  return <>{loading && <LoadingState />}{error && <div><ErrorState compact title={t('diagram.invalid')} description={error} onRetry={() => setAttempt(value => value + 1)} /><pre>{source}</pre></div>}<div className="diagram" ref={container} hidden={loading || !!error} /></>;
}
