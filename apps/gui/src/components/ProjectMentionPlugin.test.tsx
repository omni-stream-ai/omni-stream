import { describe, expect, it, vi } from 'vitest';
import { useEffect } from 'react';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { CommandSuggestions } from './CommandSuggestions';
import { createHeadlessEditor } from '@lexical/headless';
import { $createParagraphNode, $createTextNode, $getRoot, $getSelection, KEY_ARROW_DOWN_COMMAND, KEY_ENTER_COMMAND } from 'lexical';
import { LexicalComposer } from '@lexical/react/LexicalComposer';
import { ContentEditable } from '@lexical/react/LexicalContentEditable';
import { PlainTextPlugin } from '@lexical/react/LexicalPlainTextPlugin';
import { LexicalErrorBoundary } from '@lexical/react/LexicalErrorBoundary';
import { useLexicalComposerContext } from '@lexical/react/LexicalComposerContext';
import { ProjectMentionPlugin, ReferenceNode } from './ProjectMentionPlugin';

import { filterProjectOptions, slashCommandOptions } from './ProjectMentionPlugin';

describe('project mention and slash command options', () => {
  it('keeps agent identity separate from projects and supports both in one message', async () => {
    const choose = vi.fn();
    let editor: ReturnType<typeof createHeadlessEditor> | undefined;
    render(<LexicalComposer initialConfig={{ namespace: 'agent-mention-test', nodes: [ReferenceNode], onError: error => { throw error; } }}>
      <EditorCapture onEditor={value => { editor = value; }} />
      <ProjectMentionPlugin projects={['/data/code']} agents={[{ id: 7, name: 'Writer', description: 'Write reports', instructions: 'Write', engine: 'codex', plugins: null }]} onChoose={choose} />
    </LexicalComposer>);
    await waitFor(() => expect(editor).toBeDefined());
    await act(async () => { editor!.update(() => { const paragraph = $createParagraphNode().append($createTextNode('@Wri')); $getRoot().append(paragraph); paragraph.selectEnd(); }); });
    const agentOption = await screen.findByRole('option', { name: '@Writer · Agent' });
    await act(async () => { fireEvent.click(agentOption); });
    expect(choose).not.toHaveBeenCalled();
    await act(async () => { editor!.update(() => { $getRoot().selectEnd(); $getSelection()!.insertText(' @data'); }); });
    const projectOption = await screen.findByRole('option', { name: '/data/code' });
    await act(async () => { fireEvent.click(projectOption); });
    await waitFor(() => editor!.getEditorState().read(() => expect($getRoot().getAllTextNodes().filter((node): node is ReferenceNode => node instanceof ReferenceNode).map(node => node.__reference)).toEqual(['agent:7', 'project:/data/code'])));
    expect(choose).toHaveBeenCalledWith('/data/code');
  });
  it('filters projects case-insensitively without changing their order', () => {
    expect(filterProjectOptions(['/data/Code', '/home/demo'], 'code')).toEqual(['/data/Code']);
  });

  it('returns no slash options for ordinary text or multiline input', () => {
    expect(slashCommandOptions('hello')).toEqual([]);
    expect(slashCommandOptions('/Ne\n')).toEqual([]);
  });

  it('keeps the first matching slash command as the default selection', () => {
    expect(slashCommandOptions('/')).toEqual(['New', 'Clear', 'Rename']);
    expect(slashCommandOptions('/re')).toEqual(['Rename']);
  });

  it('supports slash menu keyboard navigation and selection', () => {
    const choose = vi.fn();
    render(<CommandSuggestions options={['New', 'Clear', 'Rename']} label="斜杠命令" onChoose={choose} />);
    const menu = screen.getByRole('listbox', { name: '斜杠命令' });
    expect(screen.getByRole('option', { name: '/New' })).toHaveAttribute('aria-selected', 'true');
    fireEvent.keyDown(menu, { key: 'ArrowDown' });
    expect(screen.getByRole('option', { name: '/Clear' })).toHaveAttribute('aria-selected', 'true');
    fireEvent.keyDown(menu, { key: 'Enter' });
    expect(choose).toHaveBeenCalledWith('Clear');
  });

  it('dismisses slash suggestions with Escape', () => {
    const dismiss = vi.fn();
    render(<CommandSuggestions options={['New']} label="斜杠命令" onChoose={vi.fn()} onDismiss={dismiss} />);
    fireEvent.keyDown(screen.getByRole('listbox', { name: '斜杠命令' }), { key: 'Escape' });
    expect(dismiss).toHaveBeenCalledTimes(1);
  });

  it('serializes a project reference as an atomic Lexical node', () => {
    const editor = createHeadlessEditor({ namespace: 'mention-test', nodes: [ReferenceNode] });
    editor.update(() => { $getRoot().append($createParagraphNode().append(new ReferenceNode('@/data/code', 'project:/data/code'))); });
    editor.read(() => {});
    const json = editor.getEditorState().toJSON() as unknown as { root: { children: Array<{ children: unknown[] }> } };
    const node = json.root.children[0].children[0] as { type: string; reference: string; text: string };
    expect(node.type).toBe('reference');
    expect(node.reference).toBe('project:/data/code');
    expect(node.text).toBe('@/data/code');
  });

  it('opens for a newly typed mention and closes after choosing a project', async () => {
    const choose = vi.fn();
    let editor: ReturnType<typeof createHeadlessEditor> | undefined;
    function Harness() {
      return <LexicalComposer initialConfig={{ namespace: 'mention-ui-test-2', nodes: [ReferenceNode], onError: error => { throw error; } }}>
        <EditorCapture onEditor={value => { editor = value; }} />
        <ProjectMentionPlugin projects={['/data/code', '/home/demo']} onChoose={choose} />
      </LexicalComposer>;
    }
    render(<Harness />);
    await waitFor(() => expect(editor).toBeDefined());
    editor!.update(() => { const paragraph = $createParagraphNode().append($createTextNode('@')); $getRoot().append(paragraph); paragraph.selectEnd(); });
    expect(await screen.findByRole('listbox', { name: '项目' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('option', { name: '/data/code' }));
    expect(choose).toHaveBeenCalledWith('/data/code');
    await waitFor(() => expect(screen.queryByRole('listbox', { name: '项目' })).not.toBeInTheDocument());
    editor!.update(() => { $getRoot().selectEnd(); });
    expect(screen.queryByRole('listbox', { name: '项目' })).not.toBeInTheDocument();
    editor!.dispatchCommand(KEY_ENTER_COMMAND, new KeyboardEvent('keydown', { key: 'Enter' }));
    expect(screen.queryByRole('listbox', { name: '项目' })).not.toBeInTheDocument();
  });

  it('handles Enter while the project popup is open', async () => {
    const choose = vi.fn();
    let editor: ReturnType<typeof createHeadlessEditor> | undefined;
    function Harness() {
      return <LexicalComposer initialConfig={{ namespace: 'mention-enter-test', nodes: [ReferenceNode], onError: error => { throw error; } }}>
        <EditorCapture onEditor={value => { editor = value; }} />
        <ProjectMentionPlugin projects={['/data/code']} onChoose={choose} />
      </LexicalComposer>;
    }
    render(<Harness />);
    await waitFor(() => expect(editor).toBeDefined());
    editor!.update(() => { const paragraph = $createParagraphNode().append($createTextNode('@')); $getRoot().append(paragraph); paragraph.selectEnd(); });
    await screen.findByRole('listbox', { name: '项目' });

    editor!.dispatchCommand(KEY_ENTER_COMMAND, new KeyboardEvent('keydown', { key: 'Enter' }));
    expect(choose).toHaveBeenCalledWith('/data/code');
    await waitFor(() => expect(screen.queryByRole('listbox', { name: '项目' })).not.toBeInTheDocument());
  });

  it('scrolls the selected project option into view during keyboard navigation', async () => {
    const projects = Array.from({ length: 8 }, (_, index) => `/data/project-${index}`);
    let editor: ReturnType<typeof createHeadlessEditor> | undefined;
    function Harness() {
      return <LexicalComposer initialConfig={{ namespace: 'mention-scroll-test', nodes: [ReferenceNode], onError: error => { throw error; } }}>
        <EditorCapture onEditor={value => { editor = value; }} />
        <ProjectMentionPlugin projects={projects} onChoose={vi.fn()} />
      </LexicalComposer>;
    }
    render(<Harness />);
    await waitFor(() => expect(editor).toBeDefined());
    editor!.update(() => { const paragraph = $createParagraphNode().append($createTextNode('@')); $getRoot().append(paragraph); paragraph.selectEnd(); });
    const menu = await screen.findByRole('listbox', { name: '项目' });
    const scrollIntoView = vi.fn();
    for (const option of screen.getAllByRole('option')) option.scrollIntoView = scrollIntoView;

    editor!.dispatchCommand(KEY_ARROW_DOWN_COMMAND, new KeyboardEvent('keydown', { key: 'ArrowDown' }));

    await waitFor(() => expect(scrollIntoView).toHaveBeenCalledWith({ block: 'nearest' }));
    expect(menu).toBeInTheDocument();
  });
});

function EditorCapture({ onEditor }: { onEditor: (editor: ReturnType<typeof createHeadlessEditor>) => void }) {
  const [editor] = useLexicalComposerContext();
  useEffect(() => { onEditor(editor as ReturnType<typeof createHeadlessEditor>); }, [editor, onEditor]);
  return null;
}

it('keeps path lookup failures separate from empty results and retries manually', async () => {
  const search = vi.fn().mockRejectedValueOnce(new Error('path lookup offline')).mockResolvedValue(['/data/code']);
  let editor: ReturnType<typeof createHeadlessEditor> | undefined;
  render(<LexicalComposer initialConfig={{ namespace: 'path-error-test', nodes: [ReferenceNode], onError: error => { throw error; } }}>
    <EditorCapture onEditor={value => { editor = value; }} />
    <ProjectMentionPlugin projects={[]} searchProjects={search} onChoose={vi.fn()} />
  </LexicalComposer>);
  await waitFor(() => expect(editor).toBeDefined());
  await act(async () => { editor!.update(() => { const paragraph = $createParagraphNode().append($createTextNode('@/data')); $getRoot().append(paragraph); paragraph.selectEnd(); }); });
  expect(screen.getByRole('status')).toHaveAccessibleName('加载中...');
  expect(await screen.findByRole('alert')).toHaveTextContent('path lookup offline');
  expect(screen.queryByText('没有匹配的项目')).not.toBeInTheDocument();
  expect(search).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole('button', { name: '重试' }));
  expect(await screen.findByRole('option', { name: '/data/code' })).toBeVisible();
  expect(search).toHaveBeenCalledTimes(2);
  expect(search).toHaveBeenLastCalledWith('/data');
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});
