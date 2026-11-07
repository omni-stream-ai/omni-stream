import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { VitePWA } from 'vite-plugin-pwa';

export default defineConfig({
  plugins: [
    react(),
    VitePWA({
      disable: Boolean(process.env.TAURI_ENV_PLATFORM),
      registerType: 'autoUpdate',
      includeAssets: ['favicon.ico', 'omnistream-logo.svg', 'omnistream-mark.svg', 'icons/*.png'],
      manifest: {
        name: 'OmniStream',
        short_name: 'OmniStream',
        description: 'OmniStream is a general AI assistant workspace for immediate requests in chat and recurring delegations with scheduled execution and delivered results.',
        lang: 'en-US',
        theme_color: '#101418',
        background_color: '#ffffff',
        display: 'standalone',
        start_url: '/',
        scope: '/',
        icons: [
          { src: '/icons/omnistream-192.png', sizes: '192x192', type: 'image/png' },
          { src: '/icons/omnistream-512.png', sizes: '512x512', type: 'image/png' },
          { src: '/icons/omnistream-maskable-512.png', sizes: '512x512', type: 'image/png', purpose: 'maskable' },
        ],
      },
      workbox: {
        importScripts: ['/notification-worker.js'],
        globPatterns: ['**/*.{js,css,html,ico,png,svg,woff2}'],
      },
    }),
  ],
  clearScreen: false,
  server: {
    host: process.env.TAURI_DEV_HOST || '127.0.0.1',
    port: 1420,
    strictPort: true,
    hmr: process.env.TAURI_DEV_HOST ? { protocol: 'ws', host: process.env.TAURI_DEV_HOST, port: 1421 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
  },
});
