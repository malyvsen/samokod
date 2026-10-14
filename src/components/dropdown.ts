import { useEffect, useRef, useState } from "react";

export function useDropdown() {
	const [open, setOpen] = useState(false);
	const rootRef = useRef<HTMLSpanElement>(null);

	useEffect(() => {
		if (!open) return;
		function onPointerDown(event: PointerEvent) {
			if (rootRef.current?.contains(event.target as Node)) return;
			setOpen(false);
		}
		function onKeyDown(event: KeyboardEvent) {
			if (event.key === "Escape") setOpen(false);
		}
		document.addEventListener("pointerdown", onPointerDown);
		document.addEventListener("keydown", onKeyDown);
		return () => {
			document.removeEventListener("pointerdown", onPointerDown);
			document.removeEventListener("keydown", onKeyDown);
		};
	}, [open]);

	return { open, setOpen, rootRef };
}
