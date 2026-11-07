import { useEffect, useRef, type RefObject } from 'react';
import type { NotificationSnapshot } from './api';

export type ReadNotifications = (snapshot: NotificationSnapshot) => Promise<void>;

export function useViewedNotifications(
  container: RefObject<HTMLDivElement | null>, snapshot: NotificationSnapshot | null | undefined,
  ready: boolean, onRead?: ReadNotifications,
) {
  const attempted = useRef<{ key: string; callback: ReadNotifications } | undefined>(undefined);
  useEffect(() => {
    if (!ready || !snapshot || !onRead) return;
    const element = container.current;
    if (!element) return;
    let frame = 0;
    const check = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if (document.visibilityState !== 'visible' || !document.hasFocus() || document.querySelector('dialog[open]')) return;
        const bounds = element.getBoundingClientRect();
        if (bounds.width <= 0 || bounds.height <= 0 || bounds.bottom <= 0 || bounds.top >= innerHeight || bounds.bottom > innerHeight + 1
          || element.scrollHeight - element.scrollTop - element.clientHeight >= 80) return;
        if ([...element.querySelectorAll('img')].some(image => !image.complete || image.naturalWidth === 0)) return;
        const top = document.elementFromPoint?.(Math.max(0, Math.min(innerWidth - 1, bounds.left + bounds.width / 2)), Math.min(innerHeight - 1, bounds.bottom - 2));
        if (top && !element.contains(top)) return;
        const key = JSON.stringify(snapshot);
        if (attempted.current?.key === key && attempted.current.callback === onRead) return;
        attempted.current = { key, callback: onRead };
        void onRead(snapshot);
      });
    };
    const observer = new MutationObserver(check);
    observer.observe(document.body, { attributes: true, childList: true, subtree: true, attributeFilter: ['open', 'inert', 'class'] });
    element.addEventListener('scroll', check);
    element.addEventListener('load', check, true);
    window.addEventListener('focus', check);
    window.addEventListener('resize', check);
    document.addEventListener('visibilitychange', check);
    check();
    return () => { cancelAnimationFrame(frame); observer.disconnect(); element.removeEventListener('scroll', check); element.removeEventListener('load', check, true);
      window.removeEventListener('focus', check); window.removeEventListener('resize', check); document.removeEventListener('visibilitychange', check); };
  }, [container, ready, snapshot, onRead]);
}
