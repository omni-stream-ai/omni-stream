import { isTauri, invoke } from '@tauri-apps/api/core';
import { useEffect, useState } from 'react';
import { useI18n } from '../i18n';

type Settings = { allowLan: boolean; autostart: boolean };

export function DesktopSettings() {
  const { t } = useI18n();
  const [settings, setSettings] = useState<Settings>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [changed, setChanged] = useState(false);
  const desktop = isTauri() && !/Android|iPhone|iPad/i.test(navigator.userAgent);
  async function load() {
    setBusy(true); setError('');
    try { setSettings(await invoke<Settings>('desktop_settings')); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }
  useEffect(() => { if (desktop) void load(); }, [desktop]);
  if (!desktop) return null;
  async function update(key: keyof Settings, enabled: boolean) {
    setBusy(true); setError('');
    try {
      await invoke(key === 'allowLan' ? 'set_desktop_lan' : 'set_desktop_autostart', { enabled });
      setSettings(previous => previous && { ...previous, [key]: enabled });
      if (key === 'allowLan') setChanged(true);
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }
  return <section className="dialog-form desktop-settings" aria-label={t('desktop.title')}>
    <h3>{t('desktop.title')}</h3>
    {settings ? <>
      <label><input type="checkbox" checked={settings.allowLan} disabled={busy} onChange={e => void update('allowLan', e.target.checked)} />{t('desktop.lan')}</label>
      <p className="muted">{t('desktop.lanHint')}</p>
      <label><input type="checkbox" checked={settings.autostart} disabled={busy} onChange={e => void update('autostart', e.target.checked)} />{t('desktop.autostart')}</label>
      <p className="muted">{t('desktop.autostartHint')}</p>
    </> : busy ? <p role="status">{t('common.loading')}</p> : <button type="button" onClick={() => void load()}>{t('common.retry')}</button>}
    {changed && <p role="status">{t('desktop.restart')}</p>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
