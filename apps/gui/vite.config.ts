import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { VitePWA } from 'vite-plugin-pwa';

export default defineConfig({
  plugins: [
    react(),
    VitePWA({
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
          { src: '/icons/omnistream-512.png', sizes: '512x512', type: 'image/png', purpose: 'any maskable' },
        ],
      },
      workbox: {
        globPatterns: ['**/*.{js,css,html,ico,png,svg,woff2}'],
      },
    }),
  ],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
});
