import { useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import hljs from 'highlight.js/lib/common';
import 'highlight.js/styles/github.css';
import { Check, Copy } from 'lucide-react';
import { ImagePreview, PreviewImage } from './ImagePreview';
import { Diagram } from './Diagram';
import { useI18n } from '../i18n';

function Code({ text, language }: { text: string; language: string }) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState('');
  const html = language && hljs.getLanguage(language) ? hljs.highlight(text, { language }).value : hljs.highlightAuto(text).value;
  return <span className="code-block"><span className="code-toolbar"><span>{language || 'code'}</span><button type="button" title={t('code.copy')} aria-label={t('code.copy')} onClick={() => {
    void navigator.clipboard.writeText(text).then(() => { setCopied(true); setTimeout(() => setCopied(false), 1500); }).catch(() => setError(t('code.copyFailed')));
  }}>{copied ? <Check size={14} /> : <Copy size={14} />}</button>{error && <span role="alert">{error}</span>}</span>{language === 'mermaid' ? <Diagram source={text} /> : <span className="code-content"><code dangerouslySetInnerHTML={{ __html: html }} /></span>}</span>;
}

export function MessageContent({ content, markdown = true }: { content: string; markdown?: boolean }) {
  const [preview, setPreview] = useState<PreviewImage[] | null>(null);
  const [initial, setInitial] = useState(0);
  return <><div className="rendered-message" onClick={event => {
    if (!(event.target instanceof HTMLImageElement)) return;
    const all = [...event.currentTarget.querySelectorAll('img')];
    setInitial(all.indexOf(event.target)); setPreview(all.map(img => ({ url: img.src, name: img.alt })));
  }}>
    {markdown ? <ReactMarkdown remarkPlugins={[remarkGfm]} components={{ code: ({ className, children, ...props }) => {
      const text = String(children);
      return className?.startsWith('language-') || text.includes('\n') ? <Code text={text.replace(/\n$/, '')} language={className?.replace('language-', '') ?? ''} /> : <code {...props}>{children}</code>;
    }, img: ({ ...props }) => <img {...props} tabIndex={0} onKeyDown={event => { if (event.key === 'Enter') event.currentTarget.click(); }} /> }}>{content}</ReactMarkdown> : <span>{content}</span>}
  </div>{preview && <ImagePreview images={preview} initial={initial} onClose={() => setPreview(null)} />}</>;
}
