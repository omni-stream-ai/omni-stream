import { useEffect, useRef, useState } from 'react';
import { useLexicalComposerContext } from '@lexical/react/LexicalComposerContext';
import { $createTextNode, $getRoot, $getSelection, $isRangeSelection, $isTextNode, $setSelection, COMMAND_PRIORITY_HIGH, KEY_ARROW_DOWN_COMMAND, KEY_ARROW_UP_COMMAND, KEY_ENTER_COMMAND, KEY_ESCAPE_COMMAND, NodeKey, SerializedTextNode, TextNode } from 'lexical';
import { LoadingState, ErrorState, EmptyState } from './StateView';
import { useI18n } from '../i18n';
import type { NamedAgent } from '../lib/api';

export class ReferenceNode extends TextNode {
  __reference: string;
  static getType() { return 'reference'; }
  static clone(node: ReferenceNode) { return new ReferenceNode(node.__text, node.__reference, node.__key); }
  constructor(text: string, reference: string, key?: NodeKey) { super(text, key); this.__reference = reference; }
  static importJSON(value: SerializedTextNode & { reference: string }) { return new ReferenceNode(value.text, value.reference).setMode('token'); }
  exportJSON() { return { ...super.exportJSON(), type: 'reference', reference: this.__reference }; }
  createDOM() { const element = document.createElement('span'); element.className = 'editor-reference'; element.textContent = this.__text; element.dataset.reference = this.__reference; return element; }
  updateDOM(previous: ReferenceNode, element: HTMLElement) { if (previous.__text !== this.__text) element.textContent = this.__text; return false; }
  isTextEntity() { return true; }
}

type Props = { projects: string[]; agents?: NamedAgent[]; searchProjects?: (query: string) => Promise<string[]>; onChoose: (value: string) => void };
type Choice = { label: string; reference: string; agent?: NamedAgent };

export function filterProjectOptions(projects: string[], query: string) {
  return projects.filter(value => value.toLowerCase().includes(query.toLowerCase()));
}

export function slashCommandOptions(text: string) {
  if (!text.startsWith('/') || text.includes('\n')) return [];
  return ['New', 'Clear', 'Rename'].filter(command => command.toLowerCase().startsWith(text.slice(1).toLowerCase()));
}

export function ProjectMentionPlugin({ projects, agents = [], searchProjects, onChoose }: Props) {
  const { t } = useI18n();
  const [editor] = useLexicalComposerContext();
  const [mention, setMention] = useState<string | null>(null);
  const [index, setIndex] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [retry, setRetry] = useState(0);
  const [resolvedMention, setResolvedMention] = useState<string | null>(null);
  const [pathOptions, setPathOptions] = useState<string[]>([]);
  const optionsRef = useRef<HTMLDivElement>(null);
  const ignoredText = useRef<string | null>(null);
  const pathMention = mention !== null && /^(\/|~)/.test(mention);
  const options: Choice[] = mention === null ? [] : [
    ...(!pathMention ? agents.filter(agent => `${agent.name} ${agent.description}`.toLowerCase().includes(mention.toLowerCase())).map(agent => ({ label: agent.name, reference: `agent:${agent.id}`, agent })) : []),
    ...(pathMention ? resolvedMention === mention ? pathOptions : [] : filterProjectOptions(projects, mention)).map(label => ({ label, reference: `project:${label}` })),
  ];

  useEffect(() => setIndex(0), [mention]);
  useEffect(() => {
    const selected = optionsRef.current?.querySelector<HTMLElement>('[aria-selected="true"]');
    if (selected && typeof selected.scrollIntoView === 'function') selected.scrollIntoView({ block: 'nearest' });
  }, [index, options]);
  useEffect(() => {
    if (!pathMention || !searchProjects || mention === null) { setPathOptions([]); setLoading(false); setError(''); return; }
    let active = true;
    setLoading(true); setError('');
    const timer = window.setTimeout(() => { void searchProjects(mention).then(value => { if (active) { setPathOptions(value); setResolvedMention(mention); } }).catch(cause => { if (active) { setPathOptions([]); setError(cause instanceof Error ? cause.message : String(cause)); } }).finally(() => { if (active) setLoading(false); }); }, 180);
    return () => { active = false; window.clearTimeout(timer); };
  }, [mention, pathMention, searchProjects, retry]);
  useEffect(() => editor.registerTextContentListener(() => {
    editor.getEditorState().read(() => {
      const text = $getRoot().getTextContent();
      if (ignoredText.current === text) { setMention(null); return; }
      ignoredText.current = null;
      const selection = $getSelection();
      if (!$isRangeSelection(selection) || !selection.isCollapsed() || selection.anchor.getNode() instanceof ReferenceNode) { setMention(null); return; }
      const node = selection.anchor.getNode();
      const match = node.getTextContent().slice(0, selection.anchor.offset).match(/(?:^|\s)@([^\s]*)$/);
      setMention(match ? match[1] : null);
    });
  }), [editor]);
  const choose = (value: Choice) => {
    editor.update(() => {
      const selection = $getSelection();
      if (!$isRangeSelection(selection)) return;
      const anchor = selection.anchor;
      const node = anchor.getNode();
      const start = anchor.offset - (mention?.length ?? 0) - 1;
      if (start >= 0 && $isTextNode(node)) selection.setTextNodeRange(node, start, node, anchor.offset);
      selection.insertNodes([new ReferenceNode(`@${value.label}`, value.reference).setMode('token'), $createTextNode(' ')]);
      ignoredText.current = $getRoot().getTextContent();
    });
    setMention(null); if (!value.agent) onChoose(value.label);
  };
  useEffect(() => editor.registerCommand(KEY_ENTER_COMMAND, event => {
    if (!event || event.isComposing || editor.isComposing() || mention === null || !options.length || !options[index]) return false;
    event.preventDefault(); choose(options[index]); return true;
  }, COMMAND_PRIORITY_HIGH), [editor, mention, options, index]);
  useEffect(() => {
    const down = editor.registerCommand(KEY_ARROW_DOWN_COMMAND, event => { if (mention === null || !options.length) return false; event.preventDefault(); setIndex(value => (value + 1) % options.length); return true; }, COMMAND_PRIORITY_HIGH);
    const up = editor.registerCommand(KEY_ARROW_UP_COMMAND, event => { if (mention === null || !options.length) return false; event.preventDefault(); setIndex(value => (value + options.length - 1) % options.length); return true; }, COMMAND_PRIORITY_HIGH);
    const escape = editor.registerCommand(KEY_ESCAPE_COMMAND, event => { if (mention === null) return false; event.preventDefault(); setMention(null); return true; }, COMMAND_PRIORITY_HIGH);
    return () => { down(); up(); escape(); };
  }, [editor, mention, options.length]);
  if (mention === null) return null;
  return <div ref={optionsRef} className="mention-options" role="listbox" aria-label={t(agents.length ? 'agents.references' : 'project.label')}>{options.map((value, item) => <button type="button" role="option" aria-selected={item === index} key={value.reference} onMouseDown={event => event.preventDefault()} onMouseEnter={() => setIndex(item)} onClick={() => choose(value)}>{value.agent ? `@${value.label} · ${t('agents.agent')}` : value.label}</button>)}{pathMention && loading && <LoadingState />}
    {pathMention && error && !loading && <ErrorState compact title={t('project.pathLoadFailed')} description={error} onRetry={() => setRetry(value => value + 1)} />}
    {!options.length && !loading && !error && <EmptyState compact title={t('project.noMatch')} description={t('project.noSearchResults')} />}</div>;
}
