import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import type { NotificationSnapshot } from './api';
import { putPushRecord, readStoragePrefix, receiptKey, saveReadReceipt } from './notificationReceipts';
import { dismissSystemNotifications } from './systemNotifications';
import { loadBinding, pushRequest, remoteDeviceInput } from './remotePush';

export function useNotificationReads(server: string) {
  const current = useRef(server); current.current = server;
  const pending = useRef(new Map<string, { server: string; snapshot: NotificationSnapshot }>());
  const failed = useRef(new Set<string>());
  const [error, setError] = useState<{ server: string; message: string }>();
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    const changed = (event: StorageEvent) => {
      if (!event.key?.startsWith(readStoragePrefix) || event.newValue === null) return;
      try {
        const [source, scope, instance] = JSON.parse(event.key.slice(readStoragePrefix.length));
        const cursor = Number(event.newValue);
        if (source === server && typeof scope === 'string' && typeof instance === 'string' && Number.isSafeInteger(cursor) && cursor >= 0) {
          const snapshot = { scope, instance, cursor };
          void dismissSystemNotifications(server, snapshot).catch(cause => {
            pending.current.set(JSON.stringify([server,scope]), { server, snapshot }); failed.current.add(server);
            if (current.current === server) setError({ server, message: String(cause) });
          });
        }
      } catch { /* Ignore unrelated or malformed storage events. */ }
    };
    window.addEventListener('storage', changed);
    return () => window.removeEventListener('storage', changed);
  }, [server]);

  const synchronize = useCallback(async (source: string, snapshot: NotificationSnapshot, remote: boolean) => {
    if (isTauri() && window.__OMNISTREAM_ANDROID__) {
      await invoke('remote_push', { action: 'markRead', input: { server: source, ...snapshot } });
    } else if (!isTauri() && navigator.serviceWorker) {
      await putPushRecord('reads', { key: receiptKey(source, snapshot.scope, snapshot.instance), server: source, ...snapshot });
    }
    await dismissSystemNotifications(source, snapshot);
    const binding = loadBinding(source);
    if (remote && binding?.registered && binding.enabled) {
      await pushRequest(source, '/push/devices/read', { ...remoteDeviceInput(binding), read: snapshot });
    }
  }, []);

  const read = useCallback(async (snapshot: NotificationSnapshot) => {
    const key = JSON.stringify([server, snapshot.scope]);
    const work = async () => {
      pending.current.set(key, { server, snapshot });
      try {
        const saved = saveReadReceipt(server, snapshot);
        pending.current.set(key, { server, snapshot: saved });
        const blocked = failed.current.has(server);
        await synchronize(server, saved, !blocked);
        if (blocked) return;
        if (pending.current.get(key)?.snapshot === saved) pending.current.delete(key);
      } catch (cause) {
        failed.current.add(server);
        if (current.current === server) setError({ server, message: String(cause) });
      }
    };
    if (navigator.locks) await navigator.locks.request('omnistream.notifications.read:' + receiptKey(server, snapshot.scope, snapshot.instance), work);
    else await work();
  }, [server, synchronize]);

  const retry = async () => {
    setBusy(true);
    failed.current.delete(server);
    if (current.current === server) setError(undefined);
    for (const item of [...pending.current.values()].filter(item => item.server === server)) {
      await read(item.snapshot);
      if (failed.current.has(server)) break;
    }
    if (current.current === server) setBusy(false);
  };
  return { read, error: error?.server === server ? error.message : '', busy, retry };
}
