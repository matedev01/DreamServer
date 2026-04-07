import React from 'react';
import ReactMarkdown from 'react-markdown';

/**
 * Renders markdown content with code block styling.
 * Handles streaming partial content gracefully.
 */
export default function StreamingMarkdown({ content }) {
  if (!content) return null;

  return (
    <div className="prose prose-invert prose-sm max-w-none">
      <ReactMarkdown
        components={{
          code({ node, inline, className, children, ...props }) {
            if (inline) {
              return (
                <code className="bg-gray-700 px-1.5 py-0.5 rounded text-dream-300 text-xs" {...props}>
                  {children}
                </code>
              );
            }
            const lang = className?.replace('language-', '') || '';
            return (
              <div className="relative my-2">
                {lang && (
                  <span className="absolute top-0 right-0 px-2 py-0.5 text-[10px] text-gray-400 bg-gray-800 rounded-bl">
                    {lang}
                  </span>
                )}
                <pre className="bg-gray-900 rounded-lg p-3 overflow-x-auto">
                  <code className={`text-xs leading-relaxed ${className || ''}`} {...props}>
                    {children}
                  </code>
                </pre>
              </div>
            );
          },
          p({ children }) {
            return <p className="mb-2 last:mb-0">{children}</p>;
          },
          ul({ children }) {
            return <ul className="list-disc pl-5 mb-2 space-y-1">{children}</ul>;
          },
          ol({ children }) {
            return <ol className="list-decimal pl-5 mb-2 space-y-1">{children}</ol>;
          },
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}
