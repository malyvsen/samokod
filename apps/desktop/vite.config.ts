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
		hmr: host ? { protocol: "ws", host, port: 1421 } : true,
		watch: {
			ignored: ["**/src-tauri/**"],
		},
	},
	test: {
		environment: "jsdom",
		setupFiles: ["./src/test-setup.ts"],
	},
});
