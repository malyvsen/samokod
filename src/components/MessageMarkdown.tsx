import { openUrl } from "@tauri-apps/plugin-opener";
import type { ComponentProps, MouseEvent } from "react";
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

function MarkdownLink({ href, children, ...props }: ComponentProps<"a">) {
	return (
		<a
			{...props}
			href={href}
			target="_blank"
			rel="noopener noreferrer"
			onClick={(event: MouseEvent<HTMLAnchorElement>) => {
				event.preventDefault();
				if (href && /^https?:\/\//i.test(href)) {
					openUrl(href).catch((error) => {
						console.error(`failed to open link ${href}`, error);
					});
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
