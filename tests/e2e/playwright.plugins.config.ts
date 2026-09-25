import { defineConfig, devices } from '@playwright/test';
import path from 'node:path';
import { WAIT } from './playwright.config';

const port = Number(process.env.GOOFI_PLUGIN_TEST_PORT ?? 8599);
export default defineConfig({
	testDir: './plugin-tests',
	testMatch: ['session.spec.ts', 'cables.spec.ts'],
	workers: 1,
	timeout: 5 * WAIT,
	expect: { timeout: WAIT, toPass: { timeout: WAIT } },
	use: { baseURL: `http://127.0.0.1:${port}`, actionTimeout: WAIT },
	webServer: {
		command: 'cargo run -p goofi-tests --example plugin_host',
		cwd: path.resolve(__dirname, '../..'),
		url: `http://127.0.0.1:${port}`,
		timeout: 120_000,
		reuseExistingServer: false
	},
	projects: [
		{ name: 'desktop' },
		{ name: 'phone', use: { ...devices['Pixel 7'] } }
	]
});
