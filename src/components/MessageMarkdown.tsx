import { openUrl } from "@tauri-apps/plugin-opener";
import type { ComponentProps } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

export function MessageMarkdown({ text }: { text: string }) {
	return (
		<ReactMarkdown
			remarkPlugins={[remarkGfm]}
			components={{ table: TableWrap, a: MarkdownLink }}
		>
			{text}
		</ReactMarkdown>
	);
}

// Web links open in the default external app. Anything else (relative paths,
// fragments, other schemes, missing href) stays inert: the click is swallowed
// so the Tauri webview can never navigate away.
const externalUrl = /^https?:\/\//i;

function isExternalUrl(href: string | undefined): href is string {
	return href !== undefined && externalUrl.test(href);
}

function openExternalUrl(href: string): void {
	openUrl(href).catch((error: unknown) => {
		console.error(`failed to open link ${href}`, error);
	});
}

function MarkdownLink({ href, children, ...props }: ComponentProps<"a">) {
	return (
		<a
			{...props}
			href={href}
			target="_blank"
			rel="noopener noreferrer"
			onClick={(event) => {
				event.preventDefault();
				if (isExternalUrl(href)) {
					openExternalUrl(href);
				}
			}}
		>
			{children}
		</a>
	);
}

function TableWrap({ children, ...props }: ComponentProps<"table">) {
	return (
		<div className="md-table-wrap">
			<table {...props}>{children}</table>
		</div>
	);
}
