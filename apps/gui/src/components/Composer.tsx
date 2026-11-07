import { randomId } from '../lib/uuid';
import { ErrorState } from './StateView';
import { useEffect, useRef, useState, ReactNode } from 'react';
import { LexicalComposer } from '@lexical/react/LexicalComposer';
import { PlainTextPlugin } from '@lexical/react/LexicalPlainTextPlugin';
import { ContentEditable } from '@lexical/react/LexicalContentEditable';
import { HistoryPlugin } from '@lexical/react/LexicalHistoryPlugin';
import { LexicalErrorBoundary } from '@lexical/react/LexicalErrorBoundary';
import { useLexicalComposerContext } from '@lexical/react/LexicalComposerContext';
import { $createTextNode, $getRoot, $getSelection, $setSelection, $isRangeSelection, KEY_ENTER_COMMAND, KEY_ARROW_DOWN_COMMAND, KEY_ARROW_UP_COMMAND, KEY_ESCAPE_COMMAND, PASTE_COMMAND, COMMAND_PRIORITY_HIGH, COMMAND_PRIORITY_LOW } from 'lexical';
import { File, Paperclip, Send, X } from 'lucide-react';
import { ImagePreview } from './ImagePreview';
import { ProjectMentionPlugin, ReferenceNode, slashCommandOptions } from './ProjectMentionPlugin';
import { CommandSuggestions } from './CommandSuggestions';
import { useI18n } from '../i18n';
import type { NamedAgent } from '../lib/api';

export type Attachment = { id: string; name: string; type: string; data: string; inline: boolean };

type Props = {
  busy: boolean;
  projects: string[];
  agents?: NamedAgent[];
  searchProjects?: (query: string) => Promise<string[]>;
  controls: ReactNode;
  onSend: (text: string, attachments: Attachment[], project?: string, agentId?: number) => Promise<void>;
  onCommand: (command: 'new' | 'rename') => void;
};

function Editor({ busy, projects, agents, searchProjects, controls, onSend, onCommand }: Props) {
  const { t } = useI18n();
  const [editor] = useLexicalComposerContext();
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [text, setText] = useState('');
  const [referenceIds, setReferenceIds] = useState<Set<string>>(new Set());
  const [project, setProject] = useState<string>();
  const [preview, setPreview] = useState<number | null>(null);
  const [error, setError] = useState('');
  const [sending, setSending] = useState(false);
  const [reading, setReading] = useState(0);
  const [slashIndex, setSlashIndex] = useState(0);
  const picker = useRef<HTMLInputElement>(null);
  const sequence = useRef(0);
  const imageAttachments = attachments.filter(item => item.type.startsWith('image/') && (!item.inline || referenceIds.has(item.id)));
  const slashOptions = slashCommandOptions(text);
  useEffect(() => setSlashIndex(0), [text]);
  const runSlashCommand = (command: string) => { onCommand(command === 'Rename' ? 'rename' : 'new'); editor.update(() => $getRoot().clear()); };

  useEffect(() => editor.registerUpdateListener(({ editorState }) => {
    editorState.read(() => {
      const next = $getRoot().getTextContent(); setText(next);
      setReferenceIds(new Set($getRoot().getAllTextNodes().filter((node): node is ReferenceNode => node instanceof ReferenceNode).map(node => node.__reference)));
    });
  }), [editor]);

  const insertReference = (label: string, reference: string, savedSelection: ReturnType<typeof $getSelection>) => editor.update(() => {
    if (savedSelection) $setSelection(savedSelection);
    const selection = $getSelection();
    if ($isRangeSelection(selection)) selection.insertNodes([new ReferenceNode(label, reference).setMode('token'), $createTextNode(' ')]);
  }, { discrete: true });

  const addFiles = async (files: FileList | globalThis.File[], inline: boolean) => {
    setError('');
    const incoming = Array.from(files);
    if (attachments.length + incoming.length > 20 || attachments.reduce((sum, item) => sum + item.data.length * 0.75, 0) + incoming.reduce((sum, file) => sum + file.size, 0) > 20 * 1024 * 1024) {
      setError(t('composer.tooManyAttachments')); return;
    }
    setReading(value => value + 1);
    let selection = editor.getEditorState().read(() => $getSelection()?.clone() ?? null);
    for (const file of incoming) {
      if (file.size > 10 * 1024 * 1024) { setError(t('composer.fileTooLarge', { name: file.name })); continue; }
      try {
        const data = await new Promise<string>((resolve, reject) => { const reader = new FileReader(); reader.onload = () => resolve(String(reader.result)); reader.onerror = () => reject(new Error(t('composer.readFailed'))); reader.readAsDataURL(file); });
        const id = randomId();
        const item = { id, name: file.name, type: file.type || 'application/octet-stream', data, inline };
        setAttachments(value => [...value, item]);
        if (inline) {
          insertReference(`[${file.type.startsWith('image/') ? 'Image' : 'File'}${++sequence.current}]`, id, selection);
          selection = editor.getEditorState().read(() => $getSelection()?.clone() ?? null);
        }
      } catch { setError(t('composer.cannotRead', { name: file.name })); }
    }
    setReading(value => value - 1);
  };

  const send = async () => {
    if (busy || sending || reading) return;
    const message = editor.getEditorState().read(() => $getRoot().getTextContent()).trim();
    if (/^\/(new|clear)$/i.test(message)) { onCommand('new'); return; }
    if (/^\/rename$/i.test(message)) { onCommand('rename'); editor.update(() => $getRoot().clear()); return; }
    const references = new Set<string>();
    editor.getEditorState().read(() => $getRoot().getAllTextNodes().forEach(node => { if (node instanceof ReferenceNode) references.add(node.__reference); }));
    const current = attachments.filter(item => !item.inline || references.has(item.id));
    const agentIds = [...references].filter(reference => reference.startsWith('agent:')).map(reference => Number(reference.slice(6)));
    if (agentIds.length > 1) { setError(t('agents.oneAtATime')); return; }
    if (!message && !current.length) return;
    setSending(true); setError('');
    editor.update(() => $getRoot().clear());
    setAttachments([]);
    setProject(undefined);
    try {
      await onSend(message, current, project && references.has(`project:${project}`) ? project : undefined, agentIds[0]);
    } catch { /* ConversationView renders request failures in the message stream. */ }
    finally { setSending(false); }
  };
  useEffect(() => editor.registerCommand(KEY_ENTER_COMMAND, event => {
    if (!event || event.isComposing || editor.isComposing()) return false;
    if (!event.shiftKey && slashOptions[slashIndex]) { event.preventDefault(); runSlashCommand(slashOptions[slashIndex]); return true; }
    if (event.shiftKey) return false;
    event.preventDefault(); void send(); return true;
  }, COMMAND_PRIORITY_LOW), [editor, busy, sending, reading, attachments, project, onSend, slashOptions, slashIndex]);
  useEffect(() => {
    const down = editor.registerCommand(KEY_ARROW_DOWN_COMMAND, event => { if (!slashOptions.length) return false; event.preventDefault(); setSlashIndex(index => (index + 1) % slashOptions.length); return true; }, COMMAND_PRIORITY_HIGH);
    const up = editor.registerCommand(KEY_ARROW_UP_COMMAND, event => { if (!slashOptions.length) return false; event.preventDefault(); setSlashIndex(index => (index + slashOptions.length - 1) % slashOptions.length); return true; }, COMMAND_PRIORITY_HIGH);
    const escape = editor.registerCommand(KEY_ESCAPE_COMMAND, event => { if (!slashOptions.length) return false; event.preventDefault(); editor.update(() => $getRoot().clear()); return true; }, COMMAND_PRIORITY_HIGH);
    return () => { down(); up(); escape(); };
  }, [editor, slashOptions.length]);
  useEffect(() => editor.registerCommand(PASTE_COMMAND, event => {
    const files = (event as ClipboardEvent).clipboardData?.files;
    if (!files?.length) return false;
    event.preventDefault(); void addFiles(files, true); return true;
  }, COMMAND_PRIORITY_HIGH), [editor, attachments]);

  return <div className="composer">
    {!!attachments.filter(item => !item.inline).length && <div className="attachment-strip">{attachments.filter(item => !item.inline).map(item => <div className="attachment" key={item.id}>
      <button type="button" title={item.name} aria-label={item.name} disabled={!item.type.startsWith('image/')} onClick={() => setPreview(imageAttachments.findIndex(image => image.id === item.id))}>{item.type.startsWith('image/') ? <img src={item.data} alt={item.name} /> : <File size={24} />}<span>{item.name}</span></button>
      <button type="button" className="attachment-remove" title={t('composer.removeAttachment', { name: item.name })} aria-label={t('composer.removeAttachment', { name: item.name })} onClick={() => setAttachments(value => value.filter(entry => entry.id !== item.id))}><X size={12} /></button>
    </div>)}</div>}
    <div className="editor-area" onClick={event => { const target = (event.target as HTMLElement).closest<HTMLElement>('[data-reference]'); if (target) { const index = imageAttachments.findIndex(item => item.id === target.dataset.reference); if (index >= 0) setPreview(index); } }}>
      <PlainTextPlugin contentEditable={<ContentEditable className="message-editor" aria-label={t('composer.message')} />} placeholder={<span className="editor-placeholder">{t('composer.placeholder')}</span>} ErrorBoundary={LexicalErrorBoundary} />
      <HistoryPlugin />
      <ProjectMentionPlugin projects={projects} agents={agents} searchProjects={searchProjects} onChoose={setProject} />
    </div>
    <CommandSuggestions options={slashOptions} label={t('composer.slashCommands')} onChoose={runSlashCommand} onDismiss={() => editor.update(() => $getRoot().clear())} />
    <div className="composer-toolbar"><div className="composer-settings">{controls}</div><div className="composer-actions">
      <input type="file" multiple hidden ref={picker} onChange={event => { if (event.target.files) void addFiles(event.target.files, false); event.target.value = ''; }} />
      <button type="button" className="icon-button" title={t('composer.addAttachment')} aria-label={t('composer.addAttachment')} onClick={() => picker.current?.click()}><Paperclip size={18} /></button>
      <button type="button" className="primary send-button" title={t('composer.send')} aria-label={t('composer.send')} disabled={busy || sending || !!reading || (!text.trim() && !attachments.length)} onClick={() => void send()}><Send size={18} /></button>
    </div></div>
    {error && <ErrorState compact title={t('common.operationFailed')} description={error} onRetry={() => setError('')} retryLabel={t('common.edit')} />}
    {preview !== null && <ImagePreview images={imageAttachments.map(item => ({ url: item.data, name: item.name }))} initial={preview} onClose={() => setPreview(null)} />}
  </div>;
}

export function Composer(props: Props) {
  return <LexicalComposer initialConfig={{ namespace: 'conversation', nodes: [ReferenceNode], onError: error => { throw error; } }}><Editor {...props} /></LexicalComposer>;
}
