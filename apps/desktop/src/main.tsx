import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./App.css";
import { initLogging } from "./logger";

initLogging();

const root = document.getElementById("root");
if (root === null) {
	throw new Error("index.html is missing #root");
}

createRoot(root).render(
	<StrictMode>
		<App />
	</StrictMode>,
);
