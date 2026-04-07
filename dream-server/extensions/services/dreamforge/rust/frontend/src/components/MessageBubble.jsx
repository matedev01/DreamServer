import React from 'react';
import StreamingMarkdown from './StreamingMarkdown';

export default function MessageBubble({ message }) {
  const isUser = message.role === 'user';
  const isError = message.role === 'error';

  return (
    <div className={`flex ${isUser ? 'justify-end' : 'justify-start'}`}>
      <div
        className={`max-w-3xl rounded-2xl px-4 py-3 text-sm leading-relaxed ${
          isUser
            ? 'bg-dream-600 text-white ml-12'
            : isError
            ? 'bg-red-900/40 border border-red-700 text-red-200 mr-12'
            : 'bg-gray-800 text-gray-200 mr-12'
        } ${message.streaming ? 'border-l-2 border-dream-400' : ''}`}
      >
        {isUser ? (
          <p className="whitespace-pre-wrap">{message.content}</p>
        ) : (
          <StreamingMarkdown content={message.content} />
        )}
      </div>
    </div>
  );
}
