import { defineConfig, devices } from '@playwright/test';
import path from 'node:path';
import { REPO_ROOT, WAIT } from './playwright.config';

const port = Number(process.env.GOOFI_PLUGIN_TEST_PORT ?? 8599);
export default defineConfig({
	testDir: './plugin-tests',
	testMatch: ['session.spec.ts', 'cables.spec.ts', 'latency.spec.ts'],
	workers: 1,
	timeout: 5 * WAIT,
	expect: { timeout: WAIT, toPass: { timeout: WAIT } },
	use: { baseURL: `http://127.0.0.1:${port}`, actionTimeout: WAIT },
	// The host the workspace build left, as `BIN` is: a cargo command of its own would resolve
	// features anew and rebuild the tree under the suite.
	webServer: {
		command: path.join(REPO_ROOT, 'target', 'debug', 'examples', `plugin_host${process.platform === 'win32' ? '.exe' : ''}`),
		cwd: REPO_ROOT,
		url: `http://127.0.0.1:${port}`,
		timeout: 120_000,
		reuseExistingServer: false
	},
	projects: [
		{ name: 'desktop' },
		{ name: 'phone', use: { ...devices['Pixel 7'] } }
	]
});
