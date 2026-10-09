import { useEffect, useState } from "react";
import { onAppEvent, planMdText } from "../api";
import { type SessionKey, sameSession } from "../types";

export function usePlanMd(
	selectedKey: SessionKey | null,
	working: boolean,
): string | null {
	const [text, setText] = useState<string | null>(null);

	useEffect(() => {
		if (selectedKey === null || selectedKey.role !== "scoping") {
			setText(null);
			return;
		}
		const key = selectedKey;
		let cancelled = false;
		const load = () =>
			loadPlanMd(key, (next) => {
				if (!cancelled) setText(next);
			});
		load();
		const stop = onAppEvent((event) => {
			if (event.type === "plans_changed") load();
			if (event.type === "turn_done" && sameSession(event.session, key)) {
				load();
			}
		});
		return () => {
			cancelled = true;
			stop();
		};
	}, [selectedKey]);

	useEffect(() => {
		if (!working || selectedKey === null || selectedKey.role !== "scoping") {
			return;
		}
		const key = selectedKey;
		const timer = window.setInterval(() => {
			loadPlanMd(key, setText);
		}, 2000);
		return () => {
			window.clearInterval(timer);
		};
	}, [selectedKey, working]);

	return text;
}

function loadPlanMd(key: SessionKey, onText: (text: string | null) => void) {
	void planMdText(key)
		.then(onText)
		.catch((error: unknown) => {
			console.warn("plan_md_text failed", error);
		});
}
