import { greeting } from "./greeting";

export function App() {
	return (
		<main>
			<h1>{greeting("Tauri")}</h1>
			<p>Edit this app and run checks with Task.</p>
		</main>
	);
}
