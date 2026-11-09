import {defineConfig, devices} from '@playwright/test'
// WebKit = the engine of the macOS app's WKWebView.
export default defineConfig({
  testDir: 'e2e',
  timeout: 30000,
  fullyParallel: true,
  reporter: [['list']],
  use: {baseURL: 'http://127.0.0.1:5174', viewport: {width: 1400, height: 900}, trace: 'retain-on-failure'},
  projects: [{name: 'webkit', use: {...devices['Desktop Safari'], viewport: {width: 1400, height: 900}}}],
  webServer: {command: 'npx vite --port 5174 --strictPort', url: 'http://127.0.0.1:5174', reuseExistingServer: true, timeout: 60000},
})
