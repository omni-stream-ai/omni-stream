import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  timeout: 45000,
  workers: 1,
  webServer: { command: 'pnpm dev --host 127.0.0.1 --port 1436', url: 'http://127.0.0.1:1436', reuseExistingServer: true },
  use: { baseURL: 'http://127.0.0.1:1436', screenshot: 'only-on-failure', trace: 'retain-on-failure', launchOptions: { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE } },
});
