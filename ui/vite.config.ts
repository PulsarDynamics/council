import tailwindcss from '@tailwindcss/vite';
import adapter from '@sveltejs/adapter-auto';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

const ORCHESTRATOR_URL = process.env.COUNCIL_ORCHESTRATOR_URL ?? 'http://127.0.0.1:8080';

export default defineConfig({
	plugins: [
		tailwindcss(),
		sveltekit({
			compilerOptions: {
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			adapter: adapter()
		})
	],
	server: {
		port: 5173,
		proxy: {
			// HTTP API
			'/api': {
				target: ORCHESTRATOR_URL,
				changeOrigin: true
			},
			// WebSocket — Vite needs `ws: true` and the protocol upgraded.
			'/ws': {
				target: ORCHESTRATOR_URL.replace(/^http/, 'ws'),
				ws: true,
				changeOrigin: true
			}
		}
	}
});
