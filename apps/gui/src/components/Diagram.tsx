import { useEffect, useRef, useState } from 'react';
import { useI18n } from '../i18n';

export function Diagram({ source }: { source: string }) {
  const { t } = useI18n();
  const container = useRef<HTMLDivElement>(null);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    setError('');
    void import('mermaid').then(async ({ default: mermaid }) => {
      mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', theme: 'neutral', suppressErrorRendering: true });
      const { svg } = await mermaid.render(`diagram-${crypto.randomUUID()}`, source);
      if (active && container.current) container.current.innerHTML = svg;
    }).catch(() => { if (active) setError(t('diagram.invalid')); });
    return () => { active = false; };
  }, [source, t]);
  return error ? <div><p className="error">{error}</p><pre>{source}</pre></div> : <div className="diagram" ref={container} aria-label={t('diagram.invalid')} />;
}
