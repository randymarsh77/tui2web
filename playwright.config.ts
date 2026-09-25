import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests/browser",
  timeout: 30000,
  expect: { timeout: 10000 },
  use: { baseURL: "http://localhost:8080", headless: true },
  webServer: { command: "npm run serve", url: "http://localhost:8080", reuseExistingServer: !process.env.CI },
});
