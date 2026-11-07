declare global {
  interface Window {
    __OMNISTREAM_MOBILE__?: boolean;
  }
}

// Set by the native shell before application scripts run, including on iPad.
export const isNativeMobile = () => window.__OMNISTREAM_MOBILE__ === true;
