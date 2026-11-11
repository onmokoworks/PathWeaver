import {defineConfig, devices} from '@playwright/test'
// PW_PORT lets several checkouts (git worktrees) run their tests side by side.
const port = Number(process.env.PW_PORT) || 5174
// WebKit = the engine of the macOS app's WKWebView.
export default defineConfig({
  testDir: 'e2e',
  timeout: 30000,
  fullyParallel: true,
  reporter: [['list']],
  use: {baseURL: `http://127.0.0.1:${port}`, viewport: {width: 1400, height: 900}, trace: 'retain-on-failure'},
  projects: [{name: 'webkit', use: {...devices['Desktop Safari'], viewport: {width: 1400, height: 900}}}],
  webServer: {command: `npx vite --port ${port} --strictPort`, url: `http://127.0.0.1:${port}`, reuseExistingServer: true, timeout: 60000},
})
