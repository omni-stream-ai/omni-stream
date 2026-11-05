import { useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, ChevronDown, ChevronRight, Cpu, RotateCw, X } from 'lucide-react';
import { api, ModelCatalog, ModelOption } from '../lib/api';
import { useI18n } from '../i18n';

function Skeleton() {
  const { t } = useI18n();
  return <div className="model-skeleton" role="status" aria-label={t('model.loading')}><i /><i /><i /></div>;
}

export function ModelPicker({ server }: { server: string }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const container = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [provider, setProvider] = useState<string | null>(null);
  const [model, setModel] = useState<string | null>(null);
  const catalog = useQuery({ queryKey: ['model-catalog', server], queryFn: api.modelCatalog, staleTime: 0 });
  const models = useQuery({ queryKey: ['provider-models', server, provider], queryFn: () => api.providerModels(provider!), enabled: open && !!provider, staleTime: 300000 });
  const refresh = useMutation({ mutationFn: (requestedProvider: string) => api.providerModels(requestedProvider, true), onSuccess: (result, requestedProvider) => client.setQueryData(['provider-models', server, requestedProvider], result) });
  const close = () => { setOpen(false); trigger.current?.focus(); };
  const save = useMutation({
    mutationFn: api.selectModel,
    onSuccess: selection => {
      client.setQueryData<ModelCatalog>(['model-catalog', server], previous => previous ? { ...previous, selection } : previous);
      const channel = new BroadcastChannel('model-selection'); channel.postMessage(server); channel.close();
      close();
    },
  });
  useEffect(() => {
    const channel = new BroadcastChannel('model-selection');
    channel.onmessage = event => { if (event.data === server) void client.invalidateQueries({ queryKey: ['model-catalog', server] }); };
    return () => channel.close();
  }, [client, server]);
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => { if (container.current && !container.current.contains(event.target as Node)) setOpen(false); };
    document.addEventListener('pointerdown', outside);
    return () => document.removeEventListener('pointerdown', outside);
  }, [open]);
  const selected = catalog.data?.selection;
  const label = selected?.model ? [selected.model, selected.reasoning].filter(Boolean).join(' / ') : t('model.choose');
  const reasoningLevels = models.data?.reasoning?.length ? models.data.reasoning : ['low', 'medium', 'high'];
  useEffect(() => {
    if (!open || !selected?.provider || !selected.model) return;
    const frame = requestAnimationFrame(() => {
      const root = container.current;
      const target = root?.querySelector<HTMLElement>('[data-selection-level="true"]')
        ?? root?.querySelector<HTMLElement>('[data-selection-model="true"]')
        ?? root?.querySelector<HTMLElement>('[data-selection-provider="true"]');
      target?.scrollIntoView({ block: 'nearest' });
    });
    return () => cancelAnimationFrame(frame);
  }, [open, selected?.provider, selected?.model, selected?.reasoning, models.data]);
  const choose = (option: ModelOption, reasoning: string | null) => {
    if (!provider) return;
    save.mutate({ provider, model: option.id, reasoning });
  };
  const selectedMark = (option: ModelOption, level: string | null) => selected?.provider === provider && selected.model === option.id && (selected.reasoning || null) === level;
  const retryModels = () => { refresh.reset(); void models.refetch(); };
  const error = save.error || refresh.error;
  return <div className="model-picker" ref={container} onKeyDown={event => {
    if (event.key === 'Escape' && open) { event.preventDefault(); close(); }
    if (open && ['ArrowDown', 'ArrowUp'].includes(event.key)) {
      const items = [...(container.current?.querySelectorAll<HTMLButtonElement>('.model-menu button:not(:disabled)') ?? [])];
      const index = items.indexOf(document.activeElement as HTMLButtonElement);
      event.preventDefault(); items[(index + (event.key === 'ArrowDown' ? 1 : items.length - 1) + items.length) % items.length]?.focus();
    }
  }}>
    <button ref={trigger} type="button" className="model-trigger" aria-label={t('model.choose')} title={label} aria-haspopup="dialog" aria-expanded={open} onClick={() => {
      if (!open) { setProvider(selected?.provider ?? null); setModel(selected?.model ?? null); save.reset(); }
      setOpen(value => !value);
    }}><Cpu size={14} />{catalog.isPending ? <span className="model-trigger-skeleton" aria-label={t('model.settingsLoading')} /> : <span>{label}</span>}<ChevronDown size={12} /></button>
    {open && <div className="model-menu" role="dialog" aria-label={t('model.dialog')}>
      <header><strong>{t('model.title')}</strong><button type="button" className="icon-button" aria-label={t('common.close')} title={t('common.close')} onClick={close}><X size={15} /></button></header>
      {catalog.isPending && <Skeleton />}
      {catalog.error && <div className="model-menu-error" role="alert"><span>{catalog.error.message}</span><button type="button" disabled={catalog.isFetching} onClick={() => void catalog.refetch()}>{t('common.retry')}</button></div>}
      <div className="model-menu-list">{catalog.data?.providers.map(item => <section key={item.id}>
        <button type="button" className="model-menu-row provider-row" data-selection-provider={selected?.provider === item.id ? 'true' : undefined} aria-label={`Provider ${item.name}`} aria-expanded={provider === item.id} disabled={save.isPending} onClick={() => { setProvider(provider === item.id ? null : item.id); setModel(null); refresh.reset(); }}>
          {provider === item.id ? <ChevronDown size={14} /> : <ChevronRight size={14} />}<span>{item.name}</span>{selected?.provider === item.id && <span className="model-current-dot" />}
        </button>
        {provider === item.id && <div className="model-children">
          {(models.isPending || refresh.isPending && !models.data) && <Skeleton />}
          {models.error && <div className="model-menu-error" role="alert"><span>{models.error.message}</span><button type="button" disabled={models.isFetching} onClick={retryModels}>{t('common.retry')}</button></div>}
          {models.isSuccess && !models.data.models.length && <p className="model-empty">{t('model.none')}</p>}
          {models.data?.models.map(option => <div key={option.id}>
            <button type="button" className="model-menu-row" data-selection-model={selected?.provider === provider && selected.model === option.id ? 'true' : undefined} title={option.id} aria-label={`${t('model.title')} ${option.name}`} aria-expanded={reasoningLevels.length ? model === option.id : undefined} disabled={save.isPending} onClick={() => reasoningLevels.length ? setModel(model === option.id ? null : option.id) : choose(option, null)}>
              {reasoningLevels.length ? model === option.id ? <ChevronDown size={13} /> : <ChevronRight size={13} /> : <Cpu size={13} />}<span>{option.name}</span>{selected?.provider === provider && selected.model === option.id && <Check size={13} />}
            </button>
            {model === option.id && !!reasoningLevels.length && <div className="model-children reasoning-options">
              {reasoningLevels.map(level => <button type="button" key={level} className="model-menu-row" data-selection-level={selectedMark(option, level) ? 'true' : undefined} aria-label={`${t('model.title')} ${level}`} aria-pressed={selectedMark(option, level)} disabled={save.isPending} onClick={() => choose(option, level)}><span>{level}</span>{selectedMark(option, level) && <Check size={13} />}</button>)}
            </div>}
          </div>)}
          {models.data && <button type="button" className="model-refresh" disabled={refresh.isPending || save.isPending} onClick={() => refresh.mutate(item.id)}><RotateCw size={12} className={refresh.isPending ? 'spin' : ''} />{t('model.refresh')}</button>}
        </div>}
      </section>)}</div>
      {catalog.isSuccess && !catalog.data.providers.length && <p className="model-empty">{t('model.noProvider')}</p>}
      {save.isPending && <p className="model-empty" role="status">{t('common.saving')}</p>}
      {error && <p className="model-menu-error" role="alert">{error.message}</p>}
    </div>}
  </div>;
}
