import React, { useState } from 'react';
import { useWebSocket } from './hooks/useWebSocket';
import ChatPanel from './components/ChatPanel';
import StatusBar from './components/StatusBar';
import SessionSidebar from './components/SessionSidebar';

const WS_URL = `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws`;

export default function App() {
  const ws = useWebSocket(WS_URL);
  const [bypass, setBypass] = useState(false);

  const handleBypassChange = (enabled) => {
    setBypass(enabled);
    ws.setBypass(enabled);
  };

  return (
    <div className="flex h-screen bg-gray-950">
      <SessionSidebar
        sessionId={ws.sessionId}
        sessions={ws.sessions}
        onNewSession={() => ws.send('session_create', {})}
        onClear={ws.clearMessages}
        onSwitchSession={ws.switchSession}
        onForkSession={ws.forkSession}
      />
      <div className="flex flex-col flex-1 min-w-0">
        <header className="flex items-center justify-between px-4 py-2 border-b border-gray-800 bg-gray-900/50">
          <div className="flex items-center gap-2">
            <span className="text-dream-400 font-bold text-lg">DreamForge</span>
            <span className="text-xs text-gray-500">local agentic coding</span>
          </div>
          <div className="flex items-center gap-4">
            {ws.modelName && (
              <span className="text-xs text-gray-500 font-mono">{ws.modelName}</span>
            )}
            <StatusBar
              connected={ws.connected}
              status={ws.status}
              onAbort={ws.abort}
            />
          </div>
        </header>
        <ChatPanel
          messages={ws.messages}
          status={ws.status}
          pendingPermission={ws.pendingPermission}
          onSend={ws.sendMessage}
          onPermissionResponse={ws.respondPermission}
          bypass={bypass}
          onBypassChange={handleBypassChange}
          turnStats={ws.turnStats}
        />
      </div>
    </div>
  );
}
