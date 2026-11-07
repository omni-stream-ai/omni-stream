import { useEffect, useRef, useState } from 'react';
import { api } from './api';
import { deliverNotificationOnce, listenForNotificationClicks, notificationPermission, showSystemNotification, type NotificationPermissionState } from './systemNotifications';
import { useRemotePush } from './useRemotePush';
import { useNotificationReads } from './useNotificationReads';

const preference = 'omnistream.notifications.enabled';

export function useSystemNotifications(server: string, connected: boolean, onOpen: (path: string) => void) {
  const [enabled, setEnabled] = useState(() => localStorage.getItem(preference) === 'true');
  const [permission, setPermission] = useState<NotificationPermissionState>();
  const [busy, setBusy] = useState(false);
  const [permissionError, setPermissionError] = useState('');
  const [feedError, setFeedError] = useState('');
  const [revision, setRevision] = useState(0);
  const remote = useRemotePush(server, enabled && permission === 'granted', connected);
  const reads = useNotificationReads(server);
  const remoteEnabled = remote.binding?.enabled === true && remote.binding.registered;
  const failed = useRef(false);
  const permissionRetry = useRef(false);
  const permissionFailed = useRef(false);
  const cursor = useRef<{ server: string; after: number | null; instance?: string }>({ server, after: null });
  const open = useRef(onOpen);
  open.current = onOpen;
  const currentServer = useRef(server);
  currentServer.current = server;
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void listenForNotificationClicks((path, source) => { if (source === currentServer.current) open.current(path); }).then(stop => {
      if (active) unlisten = stop; else stop();
    }).catch(error => { if (active) { failed.current = true; setFeedError(String(error)); } });
    return () => { active = false; unlisten?.(); };
  }, [revision]);

  async function checkPermission(request = false) {
    permissionRetry.current = request;
    setBusy(true); setPermissionError('');
    try {
      const status = await notificationPermission(request);
      permissionFailed.current = false;
      setPermission(status);
      if (request && status === 'granted') {
        localStorage.setItem(preference, 'true');
        setEnabled(true);
      }
    } catch (error) { permissionFailed.current = true; setPermissionError(String(error)); }
    finally { setBusy(false); }
  }
  useEffect(() => {
    void checkPermission();
    const refreshPermission = () => { if (!permissionFailed.current) void checkPermission(); };
    window.addEventListener('focus', refreshPermission);
    return () => window.removeEventListener('focus', refreshPermission);
  }, []);
  useEffect(() => {
    const changed = (event: StorageEvent) => {
      if (event.key !== preference) return;
      setEnabled(event.newValue === 'true');
      cursor.current = { server: currentServer.current, after: null };
      failed.current = false; setFeedError('');
    };
    window.addEventListener('storage', changed);
    return () => window.removeEventListener('storage', changed);
  }, []);

  async function updateEnabled(value: boolean) {
    if (value) void checkPermission(true);
    else {
      if (!(await remote.disableAll())) return;
      localStorage.setItem(preference, 'false');
      setEnabled(false);
      cursor.current = { server, after: null };
      failed.current = false; setFeedError('');
    }
  }

  useEffect(() => { cursor.current = { server, after: null }; }, [remoteEnabled, server]);

  useEffect(() => {
    if (cursor.current.server !== server) {
      cursor.current = { server, after: null };
      failed.current = false; setFeedError('');
    }
    if (!enabled || remoteEnabled || permission !== 'granted' || !connected || failed.current) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      if (!active || failed.current) return;
      try {
        const page = await api.notifications(cursor.current.after, cursor.current.instance);
        if (!active) return;
        for (const item of page.items) {
          if (!active) return;
          await deliverNotificationOnce(server, page.instance, item, async () => {
            if (!active) return false;
            await showSystemNotification(item, path => { if (currentServer.current === server) open.current(path); }, server, page.instance);
          });
          if (!active) return;
          cursor.current = { server, instance: page.instance, after: item.id };
        }
        cursor.current = { server, instance: page.instance, after: page.cursor };
        if (active) timer = setTimeout(() => void poll(), 3000);
      } catch (error) {
        if (active) { failed.current = true; setFeedError(String(error)); }
      }
    };
    void poll();
    return () => { active = false; clearTimeout(timer); };
  }, [server, connected, enabled, remoteEnabled, permission, revision]);

  return {
    enabled, permission, busy: busy || remote.busy, permissionError, feedError, updateEnabled, remote,
    read: reads.read, readError: reads.error, retryRead: reads.retry, readBusy: reads.busy,
    checkPermission: () => void checkPermission(),
    retryPermission: () => void checkPermission(permissionRetry.current),
    retry: () => { failed.current = false; setFeedError(''); setRevision(value => value + 1); },
  };
}
