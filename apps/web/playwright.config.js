import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  fullyParallel: true,
  workers: 3,
  reporter: 'list',
  use: { browserName: 'chromium' },
  projects: [
    { name: 'production', use: { baseURL: 'http://127.0.0.1:41989' } },
    { name: 'development', use: { baseURL: 'http://127.0.0.1:41988' } },
  ],
  webServer: [
    { command: 'npm run preview -- --port 41989', url: 'http://127.0.0.1:41989', reuseExistingServer: !process.env.CI },
    { command: 'npm run dev', url: 'http://127.0.0.1:41988', reuseExistingServer: !process.env.CI },
  ],
});
