"use client";
import { Prism as PrismHighlighter } from "react-syntax-highlighter";
import { oneLight } from "react-syntax-highlighter/dist/esm/styles/prism";

export function SyntaxHighlighter({ language, children }: { language: string; children: string }) {
  return (
    <PrismHighlighter
      style={oneLight}
      language={language}
      PreTag="div"
      customStyle={{ margin: 0, borderRadius: "0.5rem", fontSize: "0.8rem" }}
    >
      {children}
    </PrismHighlighter>
  );
}
