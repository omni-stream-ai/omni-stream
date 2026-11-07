import { useI18n } from '../i18n';
import type { useSystemNotifications } from '../lib/useSystemNotifications';
import { ErrorState, LoadingState } from './StateView';

export function NotificationSettings({ state }: { state: ReturnType<typeof useSystemNotifications> }) {
  const { t } = useI18n();
  const remote = state.remote;
  return <section className="dialog-form notification-settings">
    <p className="muted">{t('notifications.description')}</p>
    {state.permission === undefined && state.busy ? <LoadingState /> : <>
      <label><input type="checkbox" checked={state.enabled} disabled={state.busy || state.permission === 'unsupported'} onChange={event => state.updateEnabled(event.target.checked)} />{t('notifications.enable')}</label>
      {state.permission && <p role="status">{t(`notifications.permission.${state.permission}`)}</p>}
      {state.permission !== 'unsupported' && <button type="button" className="ghost" disabled={state.busy} onClick={state.checkPermission}>{t('notifications.recheck')}</button>}
    </>}
    <p className="muted">{t('notifications.scope')}</p>
    <p className="muted">{t('notifications.privacy')}</p>
    {state.enabled && state.permission === 'granted' && remote.supported && <div className="dialog-form">
      <p>{t('push.description')}</p>
      {!remote.status && remote.busy ? <LoadingState /> : <>
        {remote.status && !remote.status.enabled && <p className="muted">{t('push.coreDisabled')}</p>}
        {remote.status?.enabled && !remote.status.android && window.__OMNISTREAM_ANDROID__ && <p className="muted">{t('push.androidUnconfigured')}</p>}
        {remote.binding?.enabled ? <>
          <p role="status">{t('push.state.' + (remote.device?.state ?? 'pending'))}</p>
          {!remote.binding.registered && <button disabled={remote.busy} type="button" onClick={() => void remote.enable()}>{t('push.enable')}</button>}
          {remote.device?.state === 'paused' && <ErrorState compact title={t('push.paused')} description={t('push.error.' + remote.device.error)} onRetry={remote.retryDelivery} retrying={remote.busy} />}
          {(remote.device?.state === 'disabled' || remote.device?.state === 'unregistered') && <button disabled={remote.busy} type="button" onClick={() => void remote.enable()}>{t('push.rebind')}</button>}
          <button disabled={remote.busy} type="button" className="ghost" onClick={() => void remote.disable()}>{t('push.disable')}</button>
        </> : remote.status?.enabled && (!window.__OMNISTREAM_ANDROID__ || remote.status.android) && <>
          <button disabled={remote.busy} type="button" onClick={() => void remote.enable()}>{t('push.enable')}</button>
        </>}
        <button disabled={remote.busy} type="button" className="ghost" onClick={remote.recheck}>{t('push.recheck')}</button>
      </>}
      {remote.error && <ErrorState compact title={t('push.failed')} description={remote.error} onRetry={remote.retry} retrying={remote.busy} />}
    </div>}
    {state.permissionError && <ErrorState compact title={t('notifications.permissionFailed')} description={state.permissionError} onRetry={state.retryPermission} retrying={state.busy} />}
    {state.feedError && <ErrorState compact title={t('notifications.paused')} description={state.feedError} onRetry={state.retry} retrying={!state.enabled || state.permission !== 'granted'} />}
    {state.readError && <ErrorState compact title={t('notifications.readFailed')} description={state.readError} onRetry={state.retryRead} retrying={state.readBusy} />}
  </section>;
}
