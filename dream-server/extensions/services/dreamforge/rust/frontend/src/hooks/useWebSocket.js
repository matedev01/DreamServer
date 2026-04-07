import { useState, useEffect, useCallback, useRef } from 'react';

/**
 * WebSocket hook for DreamForge server communication.
 * Auto-connects, handles reconnection, and dispatches messages.
 */
export function useWebSocket(url) {
  const [connected, setConnected] = useState(false);
  const [sessionId, setSessionId] = useState(null);
  const [messages, setMessages] = useState([]);
  const [status, setStatus] = useState('idle');
  const [pendingPermission, setPendingPermission] = useState(null);
  const [modelName, setModelName] = useState('');
  const [permissionMode, setPermissionMode] = useState('default');
  const [sessions, setSessions] = useState([]); // session history
  const [turnStats, setTurnStats] = useState(null); // { startTime, tokensIn, tokensOut, iterations }
  const wsRef = useRef(null);
  const reconnectTimer = useRef(null);
  const isNewSessionRef = useRef(false);
  const bypassRef = useRef(false);

  const connect = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN) return;

    const ws = new WebSocket(url);
    wsRef.current = ws;

    ws.onopen = () => {
      setConnected(true);
      setStatus('idle');
      console.log('[WS] connected');
    };

    ws.onclose = () => {
      setConnected(false);
      setStatus('disconnected');
      console.log('[WS] disconnected, reconnecting in 3s...');
      reconnectTimer.current = setTimeout(connect, 3000);
    };

    ws.onerror = (e) => {
      console.error('[WS] error', e);
    };

    ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        handleMessage(msg);
      } catch (e) {
        console.error('[WS] parse error', e);
      }
    };
  }, [url]);

  const handleMessage = useCallback((msg) => {
    switch (msg.type) {
      case 'session_info': {
        const newId = msg.session_id || msg.data?.id;
        const forkedFrom = msg.data?.forked_from || null;
        // If this is a forked session, add it to the list with branch indicator
        if (forkedFrom && newId) {
          setSessions(list => {
            if (list.some(s => s.id === newId)) return list;
            return [{
              id: newId,
              title: `Fork of ${forkedFrom.slice(0, 8)}...`,
              timestamp: Date.now(),
              messageCount: 0,
              forkedFrom,
            }, ...list].slice(0, 30);
          });
        }
        // If switching to a new session, archive the old one
        if (isNewSessionRef.current && newId) {
          setSessionId(prev => {
            if (prev && prev !== newId) {
              setSessions(list => {
                if (list.some(s => s.id === prev)) return list;
                const firstUserMsg = messages.find(m => m.role === 'user');
                const msgCount = messages.length;
                return [{
                  id: prev,
                  title: firstUserMsg?.content?.slice(0, 50) || 'Untitled',
                  timestamp: Date.now(),
                  messageCount: msgCount,
                }, ...list].slice(0, 30);
              });
            }
            return newId;
          });
          setMessages([]);
          setStatus('idle');
          isNewSessionRef.current = false;
        } else {
          setSessionId(newId);
        }
        if (msg.data?.model) setModelName(msg.data.model);
        if (msg.data?.permission_mode) setPermissionMode(msg.data.permission_mode);
        break;
      }
      case 'assistant_text':
        setMessages(prev => {
          const last = prev[prev.length - 1];
          if (last?.role === 'assistant' && last?.streaming) {
            return [
              ...prev.slice(0, -1),
              { ...last, content: last.content + (msg.data?.delta || '') },
            ];
          }
          return [
            ...prev,
            { role: 'assistant', content: msg.data?.delta || '', streaming: true },
          ];
        });
        break;
      case 'assistant_text_done':
        setMessages(prev => {
          const last = prev[prev.length - 1];
          if (last?.role === 'assistant') {
            return [...prev.slice(0, -1), { ...last, streaming: false }];
          }
          return prev;
        });
        break;
      case 'tool_call_start':
        setMessages(prev => [
          ...prev,
          {
            role: 'tool',
            type: 'start',
            toolName: msg.data?.tool_name,
            toolCallId: msg.data?.tool_call_id,
            args: msg.data?.arguments,
          },
        ]);
        break;
      case 'tool_call_result':
        setMessages(prev => [
          ...prev,
          {
            role: 'tool',
            type: 'result',
            toolName: msg.data?.tool_name,
            toolCallId: msg.data?.tool_call_id,
            output: msg.data?.output,
            isError: msg.data?.is_error,
          },
        ]);
        break;
      case 'permission_request':
        if (bypassRef.current) {
          const reqId = msg.data?.request_id;
          if (reqId && wsRef.current?.readyState === WebSocket.OPEN) {
            wsRef.current.send(JSON.stringify({
              type: 'permission_response',
              data: { request_id: reqId, allow: true },
              session_id: sessionId,
            }));
          }
        } else {
          setPendingPermission(msg.data);
          setStatus('awaiting_permission');
        }
        break;
      case 'status':
        if (msg.data?.status === 'running') {
          setTurnStats({ startTime: Date.now(), tokensIn: 0, tokensOut: 0, iterations: 0 });
        }
        setStatus(msg.data?.status || 'unknown');
        break;
      case 'turn_complete':
        setTurnStats(prev => prev ? {
          ...prev,
          tokensIn: (prev.tokensIn || 0) + (msg.data?.tokens_in || 0),
          tokensOut: (prev.tokensOut || 0) + (msg.data?.tokens_out || 0),
          iterations: (prev.iterations || 0) + 1,
        } : null);
        break;
      case 'query_complete':
        setStatus('idle');
        setTurnStats(null);
        break;
      case 'error':
        setMessages(prev => [
          ...prev,
          { role: 'error', content: msg.data?.message || 'Unknown error' },
        ]);
        setStatus('idle');
        setTurnStats(null);
        break;
      case 'token_usage':
      case 'heartbeat':
        break;
      default:
        console.log('[WS] unhandled:', msg.type, msg.data);
    }
  }, []);

  useEffect(() => {
    connect();
    return () => {
      clearTimeout(reconnectTimer.current);
      wsRef.current?.close();
    };
  }, [connect]);

  const send = useCallback((type, data = {}) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      if (type === 'session_create') isNewSessionRef.current = true;
      wsRef.current.send(JSON.stringify({ type, data, session_id: sessionId }));
    }
  }, [sessionId]);

  const sendMessage = useCallback((content) => {
    setMessages(prev => [...prev, { role: 'user', content }]);
    send('user_message', { content });
  }, [send]);

  const respondPermission = useCallback((requestId, allow) => {
    send('permission_response', { request_id: requestId, allow });
    setPendingPermission(null);
    setStatus('running');
  }, [send]);

  const abort = useCallback(() => {
    send('abort');
    setStatus('idle');
    setTurnStats(null);
  }, [send]);

  const clearMessages = useCallback(() => {
    setMessages([]);
  }, []);

  const setBypass = useCallback((enabled) => {
    bypassRef.current = enabled;
    const newMode = enabled ? 'full_auto' : 'accept_edits';
    setPermissionMode(newMode);
    send('mode_change', { mode: newMode });
  }, [send]);

  const switchSession = useCallback((targetId) => {
    send('session_switch', { session_id: targetId });
    setMessages([]);
    setStatus('idle');
  }, [send]);

  const forkSession = useCallback((sourceSessionId) => {
    send('session_fork', { source_session_id: sourceSessionId });
  }, [send]);

  return {
    connected,
    sessionId,
    messages,
    status,
    pendingPermission,
    modelName,
    permissionMode,
    sessions,
    turnStats,
    sendMessage,
    respondPermission,
    abort,
    clearMessages,
    send,
    setBypass,
    switchSession,
    forkSession,
  };
}
