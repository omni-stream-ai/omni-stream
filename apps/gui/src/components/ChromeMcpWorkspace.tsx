import { useEffect, useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import { ArrowLeft, Chrome, LoaderCircle } from 'lucide-react';
import { pluginApi, type MarketplacePlugin } from '../lib/api';
import { ErrorState } from './StateView';
import { useI18n } from '../i18n';

export function ChromeMcpWorkspace({ plugin, onBack, onChange, onRefresh }: { plugin: MarketplacePlugin; onBack: () => void; onChange: (plugin: MarketplacePlugin) => void; onRefresh: () => void }) {
  const { t, locale } = useI18n();
  const [enabled, setEnabled] = useState(plugin.browser?.enabled ?? false);
  useEffect(() => setEnabled(plugin.browser?.enabled ?? false), [plugin.browser?.enabled]);
  const save = useMutation({ mutationFn: () => pluginApi.setBrowserEnabled(enabled), retry: false, onSuccess: onChange });
  const test = useMutation({ mutationFn: () => pluginApi.test(plugin.manifest.id), retry: false, onSuccess: onChange, onError: onRefresh });
  const pending = save.isPending || test.isPending;
  const error = save.error ?? test.error;
  return <section className="plugin-workspace" aria-label={t('chrome.title')}>
    <button type="button" className="ghost plugin-back" onClick={onBack}><ArrowLeft size={14} />{t('plugins.back')}</button>
    <div className="plugin-workspace-heading"><Chrome size={24} /><h3>{t('chrome.title')}</h3><span className="plugin-connection-status">{t('chrome.state.' + (plugin.browser?.status ?? 'disabled'))}</span></div>
    <p className="plugin-token-note muted">{t('chrome.description')}</p>
    <ol className="chrome-setup-steps"><li>{t('chrome.openBrowser')}</li><li>{t('chrome.enableDebugging')} <code>chrome://inspect/#remote-debugging</code></li><li>{t('chrome.allowConnection')}</li></ol>
    <form className="dialog-form plugin-connection-form" onSubmit={event => { event.preventDefault(); test.reset(); save.mutate(); }}>
      <label className="plugin-checkbox"><input type="checkbox" checked={enabled} disabled={pending} onChange={event => { setEnabled(event.target.checked); save.reset(); }} />{t('chrome.enabled')}</label>
      <p className="muted">{t('chrome.scope')}</p>
      <p className="muted">{t('chrome.credentials')}</p>
      <div className="plugin-connection-actions">
        <button type="submit" className="primary" disabled={pending}>{save.isPending && <LoaderCircle size={14} className="spin" />}{t(save.isPending ? 'common.saving' : 'common.save')}</button>
        <button type="button" className="ghost" disabled={pending || !plugin.browser?.enabled} onClick={() => { save.reset(); test.mutate(); }}>{test.isPending && <LoaderCircle size={14} className="spin" />}{t(test.isPending ? 'chrome.checking' : 'chrome.check')}</button>
      </div>
      {save.isSuccess && <p className="plugin-success" role="status">{t('chrome.saved')}</p>}
      {test.isSuccess && <p className="plugin-success" role="status">{t('chrome.connected')}</p>}
      {(error || plugin.browser?.last_error) && <ErrorState compact title={t('common.operationFailed')} description={error?.message ?? plugin.browser?.last_error ?? ''} onRetry={() => save.error ? save.mutate() : test.mutate()} retrying={pending} />}
      {plugin.browser?.verified_at && <p className="muted">{t('plugins.verifiedAt', { time: new Date(plugin.browser.verified_at * 1000).toLocaleString(locale) })}</p>}
      <details className="plugin-manual-setup"><summary>{t('chrome.details')}</summary><p className="muted">{t('chrome.runtime', { version: plugin.browser?.package_version ?? plugin.manifest.version })}</p><p className="muted">{t('chrome.lifecycle')}</p></details>
    </form>
  </section>;
}
