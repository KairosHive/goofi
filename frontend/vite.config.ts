import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vitest/config';

const BRIDGE = process.env.GOOFI_BRIDGE ?? 'http://127.0.0.1:8000';

export default defineConfig({
	plugins: [sveltekit()],
	server: {
		port: 5173,
		strictPort: false,
		proxy: {
			'/control': { target: BRIDGE, ws: true, changeOrigin: true },
			'/data': { target: BRIDGE, ws: true, changeOrigin: true },
			'/api': { target: BRIDGE, changeOrigin: true }
		}
	},
	test: {
		projects: [
			{
				// A rune test runs on Svelte's CLIENT runtime, where an effect runs and a derived is
				// cached: what a test of the store's reactivity has to see, and what a DOM environment
				// selects. Nothing here mounts a component.
				extends: true,
				resolve: { conditions: ['browser'] },
				test: { name: 'runes', environment: 'happy-dom', include: ['src/**/*.svelte.test.ts'] }
			},
			{
				extends: true,
				test: { name: 'plain', environment: 'node', include: ['src/**/*.test.ts'], exclude: ['src/**/*.svelte.test.ts'] }
			}
		]
	}
});
