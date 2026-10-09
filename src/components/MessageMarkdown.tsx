import type { ComponentProps } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

export function MessageMarkdown({ text }: { text: string }) {
	return (
		<ReactMarkdown
			remarkPlugins={[remarkGfm]}
			components={{ table: TableWrap }}
		>
			{text}
		</ReactMarkdown>
	);
}

function TableWrap({ children, ...props }: ComponentProps<"table">) {
	return (
		<div className="md-table-wrap">
			<table {...props}>{children}</table>
		</div>
	);
}
