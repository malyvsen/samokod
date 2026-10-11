import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
	plugins: [react()],
	// Keep Rust build errors on screen.
	clearScreen: false,
	server: {
		// Tauri looks for the development server on exactly this port.
		port: 1420,
		strictPort: true,
		host: host || false,
		// Frozen for self-development: plans landing on disk must not reload
		// the window and destroy in-progress plan state. Restart task dev
		// manually when idle to pick up changes.
		hmr: false,
		watch: null,
	},
	test: {
		environment: "jsdom",
		setupFiles: ["./src/test-setup.ts"],
		exclude: [
			"**/node_modules/**",
			"**/dist/**",
			"**/.samokod/worktrees/**",
			"**/.{idea,git,cache,output,temp}/**",
			"**/{karma,rollup,webpack,vite,vitest,jest,ava,babel,nyc,cypress,tsup,build}.config.*",
		],
	},
});
