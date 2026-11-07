import { useEffect, useState } from 'react';
import { accessChange, clientCredentialKey, loadClientCredential, normalizeCoreUrl } from './clientCredentials';

export function useClientCredential(server: string) {
  const [saved, setSaved] = useState(() => ({ server, credential: loadClientCredential(server) }));
  useEffect(() => {
    const update = () => setSaved({ server, credential: loadClientCredential(server) });
    const change = (event: Event) => {
      if ((event as CustomEvent<string>).detail === normalizeCoreUrl(server)) update();
    };
    const storage = (event: StorageEvent) => { if (!event.key || event.key === clientCredentialKey(server)) update(); };
    update();
    window.addEventListener(accessChange, change);
    window.addEventListener('storage', storage);
    return () => { window.removeEventListener(accessChange, change); window.removeEventListener('storage', storage); };
  }, [server]);
  return saved.server === server ? saved.credential : loadClientCredential(server);
}
