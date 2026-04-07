import React from 'react';

export default function StatusBar({ connected, status, onAbort }) {
  const statusColor = {
    idle: 'text-gray-500',
    running: 'text-dream-400',
    compacting: 'text-yellow-400',
    awaiting_permission: 'text-amber-400',
    aborted: 'text-red-400',
    disconnected: 'text-red-500',
  }[status] || 'text-gray-500';

  const statusLabel = {
    idle: 'Ready',
    running: 'Working...',
    compacting: 'Compacting...',
    awaiting_permission: 'Awaiting permission',
    aborted: 'Aborted',
    disconnected: 'Disconnected',
  }[status] || status;

  return (
    <div className="flex items-center gap-3 text-xs">
      <div className="flex items-center gap-1.5">
        <span className={`w-2 h-2 rounded-full ${connected ? 'bg-green-500' : 'bg-red-500'} ${status === 'running' ? 'status-pulse' : ''}`} />
        <span className={statusColor}>{statusLabel}</span>
      </div>
      {status === 'running' && (
        <button
          onClick={onAbort}
          className="px-2 py-0.5 rounded bg-red-900 hover:bg-red-800 text-red-300 text-xs transition-colors"
        >
          Stop
        </button>
      )}
    </div>
  );
}
