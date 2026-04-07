import React, { useState, useEffect } from 'react';

export default function ActivityIndicator({ turnStats, status }) {
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    if (!turnStats?.startTime) {
      setElapsed(0);
      return;
    }
    setElapsed(Math.floor((Date.now() - turnStats.startTime) / 1000));
    const interval = setInterval(() => {
      setElapsed(Math.floor((Date.now() - turnStats.startTime) / 1000));
    }, 1000);
    return () => clearInterval(interval);
  }, [turnStats?.startTime]);

  if (status !== 'running' && status !== 'compacting') return null;

  const mins = Math.floor(elapsed / 60);
  const secs = elapsed % 60;
  const timeStr = mins > 0 ? `${mins}m ${secs}s` : `${secs}s`;

  return (
    <div className="flex justify-start">
      <div className="bg-gray-800/80 border border-gray-700 rounded-2xl rounded-bl-sm px-4 py-3 flex items-center gap-3 max-w-md">
        {/* Animated spinner */}
        <div className="relative w-5 h-5 flex-shrink-0">
          <div className="absolute inset-0 rounded-full border-2 border-gray-700" />
          <div className="absolute inset-0 rounded-full border-2 border-dream-400 border-t-transparent animate-spin" />
        </div>
        {/* Stats */}
        <div className="flex flex-col gap-0.5">
          <div className="flex items-center gap-3 text-xs">
            <span className="text-gray-300 font-medium">
              {status === 'compacting' ? 'Compacting context...' : 'Working...'}
            </span>
            <span className="text-gray-500 font-mono">{timeStr}</span>
          </div>
          {turnStats && (turnStats.tokensIn > 0 || turnStats.iterations > 0) && (
            <div className="flex items-center gap-2 text-xs text-gray-500">
              {turnStats.iterations > 0 && (
                <span>{turnStats.iterations} turn{turnStats.iterations !== 1 ? 's' : ''}</span>
              )}
              {turnStats.tokensIn > 0 && (
                <span>{(turnStats.tokensIn / 1000).toFixed(1)}k in</span>
              )}
              {turnStats.tokensOut > 0 && (
                <span>{(turnStats.tokensOut / 1000).toFixed(1)}k out</span>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
