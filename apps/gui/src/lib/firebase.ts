import { getApp, getApps, initializeApp } from 'firebase/app';
import { initializeAnalytics as createAnalytics, isSupported, logEvent } from 'firebase/analytics';

// Public client identifiers, downloaded from the registered Firebase Web app.
export const firebaseApp = getApps().length ? getApp() : initializeApp({
  projectId: 'omnistream-9ed10',
  appId: '1:618055154194:web:b65e59b9e66799ec5a36cb',
  storageBucket: 'omnistream-9ed10.firebasestorage.app',
  apiKey: 'AIzaSyAUltIXB6rJxzFu17IHPSxclUCf6-yve5A',
  authDomain: 'omnistream-9ed10.firebaseapp.com',
  messagingSenderId: '618055154194',
  measurementId: 'G-JWPR1FVFBK',
});

// Keep conversation IDs, Core addresses, query strings and page titles out of
// telemetry. Automatic page views would include those values in this app.
export async function initializeAnalytics() {
  if (!import.meta.env.PROD) return;
  try {
    if (!await isSupported()) return;
    const analytics = createAnalytics(firebaseApp, {
      config: {
        send_page_view: false,
        page_location: window.location.origin + '/',
        page_referrer: '',
        page_title: 'OmniStream',
      },
    });
    logEvent(analytics, 'app_open', {
      page_location: window.location.origin + '/',
      page_referrer: '',
      page_title: 'OmniStream',
    });
  } catch {
    // Offline devices and blocked telemetry must not prevent startup.
  }
}
