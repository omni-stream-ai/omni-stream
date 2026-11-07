import { useEffect, useRef, useState } from 'react';
import { allBindings, bindingKey, disableRemote, loadBinding, newBinding, pushRequest, registerRemote, remoteDeviceInput, remotePushSupported, saveBinding, type PushBinding, type PushDeviceStatus, type PushStatus } from './remotePush';

export function useRemotePush(server: string, enabled: boolean, connected: boolean) {
  const supported = remotePushSupported();
  const [binding, setBinding] = useState(() => loadBinding(server));
  const [status, setStatus] = useState<PushStatus>();
  const [device, setDevice] = useState<PushDeviceStatus>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  const current = useRef(server);
  current.current = server;
  const stopped = useRef(false);
  const executing = useRef(false);
  const runId = useRef(0);
  const operation = useRef<() => Promise<void>>(async () => {});

  useEffect(() => {
    setBinding(loadBinding(server)); setStatus(undefined); setDevice(undefined); setError(''); setBusy(false); stopped.current = false;
    const changed = (event: Event) => {
      if (event instanceof StorageEvent && event.key !== bindingKey(server)) return;
      setBinding(loadBinding(server));
    };
    window.addEventListener('storage', changed);
    window.addEventListener('omnistream-push-change', changed);
    return () => { window.removeEventListener('storage', changed); window.removeEventListener('omnistream-push-change', changed); };
  }, [server]);

  async function run(work: () => Promise<void>): Promise<boolean> {
    const source = server;
    const id = ++runId.current;
    executing.current = true;
    operation.current = work;
    stopped.current = false;
    setBusy(true); setError('');
    try { await work(); return true; }
    catch (failure) { if (current.current === source && runId.current === id) { stopped.current = true; setError(String(failure)); } return false; }
    finally { if (runId.current === id) { executing.current = false; if (current.current === source) setBusy(false); } }
  }
  useEffect(() => {
    if (!supported || !enabled || !connected || stopped.current) return;
    let active = true;
    let synchronized = false;
    let timer: ReturnType<typeof setTimeout>;
    const refresh = async () => {
      if (!active || stopped.current) return;
      const info = await pushRequest<PushStatus>(server, '/push/status');
      if (!active) return;
      setStatus(info);
      let saved = loadBinding(server);
      const transport = saved?.transport ?? (window.__OMNISTREAM_ANDROID__ === true ? 'android' : 'web');
      const available = info.enabled && (transport === 'android' ? info.android : info.web);
      if (available && (!saved || (saved.enabled && !saved.registered))) {
        saved ??= newBinding(server);
        saveBinding(saved);
        const result = await registerRemote(saved, info);
        synchronized = true;
        if (active) setDevice(result);
      } else if (saved?.enabled && saved.registered && info.enabled) {
        let result = await pushRequest<PushDeviceStatus>(server, '/push/devices/status', remoteDeviceInput(saved));
        if (!synchronized && result.state === 'active') {
          result = await registerRemote(saved, info);
          synchronized = true;
        }
        if (active) setDevice(result);
      }
      if (active) timer = setTimeout(poll, 15000);
    };
    function poll() {
      if (!active || stopped.current) return;
      if (executing.current) timer = setTimeout(poll, 1000);
      else void run(refresh);
    }
    void run(refresh);
    return () => { active = false; clearTimeout(timer); };
  }, [server, enabled, connected, supported, revision]);

  async function enable() {
    return run(async () => {
      const info = status ?? await pushRequest<PushStatus>(server, '/push/status');
      const next: PushBinding = { ...(loadBinding(server) ?? newBinding(server)), enabled: true,
        locale: localStorage.getItem('omnistream.locale') === 'en-US' ? 'en-US' : 'zh-CN' };
      saveBinding(next);
      const result = await registerRemote(next, info, true);
      if (current.current === server) { setDevice(result); setStatus(info); }
    });
  }
  async function disable() {
    const saved = loadBinding(server);
    if (!saved?.enabled) return true;
    return run(async () => { await disableRemote(saved); if (current.current === server) setDevice(undefined); });
  }
  async function retryDelivery() {
    const saved = loadBinding(server);
    if (!saved) return;
    await run(async () => {
      const info = await pushRequest<PushStatus>(server, '/push/status');
      const registered = await registerRemote(saved, info);
      const result = registered.state === 'paused' ? await pushRequest<PushDeviceStatus>(server, '/push/devices/retry', remoteDeviceInput(saved)) : registered;
      if (current.current === server) setDevice(result);
    });
  }
  async function disableAll() {
    const active = allBindings().filter(item => item.enabled);
    if (!active.length) return true;
    return run(async () => {
      for (const saved of active) {
        try { await disableRemote(saved); }
        catch (failure) { throw new Error(saved.server + ': ' + String(failure)); }
      }
      if (current.current === server) setDevice(undefined);
    });
  }
  return { supported, binding: binding?.server === server ? binding : loadBinding(server), status, device, busy, error,
    enable, disable, disableAll, retryDelivery,
    recheck: () => { stopped.current = false; setRevision(value => value + 1); },
    retry: () => void run(operation.current),
  };
}
