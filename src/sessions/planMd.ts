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
		function fetch() {
			planMdText(key)
				.then((next) => {
					if (cancelled) return;
					setText(next);
				})
				.catch((error: unknown) => {
					if (cancelled) return;
					console.warn("plan_md_text failed", error);
				});
		}
		fetch();
		return () => {
			cancelled = true;
		};
	}, [selectedKey]);

	useEffect(() => {
		if (selectedKey === null || selectedKey.role !== "scoping") return;
		const key = selectedKey;
		return onAppEvent((event) => {
			if (event.type === "plans_changed") {
				void planMdText(key)
					.then((next) => setText(next))
					.catch((error: unknown) => {
						console.warn("plan_md_text failed", error);
					});
				return;
			}
			if (event.type === "turn_done" && sameSession(event.session, key)) {
				void planMdText(key)
					.then((next) => setText(next))
					.catch((error: unknown) => {
						console.warn("plan_md_text failed", error);
					});
			}
		});
	}, [selectedKey]);

	useEffect(() => {
		if (!working || selectedKey === null || selectedKey.role !== "scoping") {
			return;
		}
		const key = selectedKey;
		const timer = window.setInterval(() => {
			void planMdText(key)
				.then((next) => setText(next))
				.catch((error: unknown) => {
					console.warn("plan_md_text failed", error);
				});
		}, 2000);
		return () => {
			window.clearInterval(timer);
		};
	}, [selectedKey, working]);

	return text;
}
