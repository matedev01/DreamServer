import React, { useState, useMemo } from 'react';

const IMAGE_EXTENSIONS = ['.png', '.jpg', '.jpeg', '.gif', '.webp', '.svg', '.bmp'];
const FILE_TOOLS = ['write_file', 'edit_file', 'create_file'];
const BASH_TOOLS = ['bash', 'execute_command', 'run_command', 'shell'];

function isImagePath(str) {
  if (!str) return false;
  const lower = str.trim().toLowerCase();
  return IMAGE_EXTENSIONS.some((ext) => lower.endsWith(ext));
}

function extractImagePaths(text) {
  if (!text) return [];
  const matches = text.match(/[\w/\\._-]+\.(png|jpg|jpeg|gif|webp)/gi);
  return matches || [];
}

function extractFilename(args) {
  if (!args) return null;
  let a = args;
  if (typeof a === 'string') {
    try { a = JSON.parse(a); } catch { return null; }
  }
  return a.path || a.file_path || a.filename || null;
}

function extractFilePathFromOutput(output) {
  try {
    const parsed = JSON.parse(output);
    return parsed.file_path || parsed.path || null;
  } catch {
    return null;
  }
}

/* ── Reusable sub-components ──────────────────────────────────────────── */

function FilenameBadge({ filePath, color = 'blue' }) {
  if (!filePath) return null;
  const colors = {
    blue: 'bg-blue-900/50 text-blue-300 border-blue-700/50',
    green: 'bg-green-900/50 text-green-300 border-green-700/50',
    purple: 'bg-purple-900/50 text-purple-300 border-purple-700/50',
  };
  return (
    <span className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-full border text-[10px] font-mono ${colors[color] || colors.blue}`}>
      <svg className="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
        <polyline points="14 2 14 8 20 8" />
      </svg>
      {filePath.split('/').pop()}
    </span>
  );
}

function BashOutput({ output }) {
  const [showAll, setShowAll] = useState(false);
  const lines = output.split('\n');
  const truncated = lines.length > 20 && !showAll;
  const displayed = truncated ? lines.slice(0, 20).join('\n') : output;

  return (
    <div>
      <div className="rounded-lg bg-gray-950 p-3 overflow-x-auto max-h-80 overflow-y-auto border border-gray-800">
        <pre className="font-mono text-xs text-green-400 whitespace-pre-wrap break-all">
          {displayed}
          {truncated && '\n...'}
        </pre>
      </div>
      {lines.length > 20 && (
        <button
          onClick={() => setShowAll(!showAll)}
          className="mt-1 text-xs text-gray-500 hover:text-gray-300 transition-colors"
        >
          {showAll ? 'Show less' : `Show all ${lines.length} lines`}
        </button>
      )}
    </div>
  );
}

function ImagePreview({ path }) {
  const src = path.startsWith('/') ? `/workspace${path}` : `/workspace/${path}`;
  return (
    <div className="mt-2">
      <img
        src={src}
        alt={path}
        className="max-w-full max-h-72 rounded-lg border border-gray-700"
        loading="lazy"
        onError={(e) => { e.target.style.display = 'none'; }}
      />
      <p className="text-[10px] text-gray-500 mt-0.5 font-mono truncate">{path}</p>
    </div>
  );
}

function GenerateImageOutput({ output }) {
  const paths = extractImagePaths(output);
  // Also try JSON parsing for file_path
  let jsonPath = null;
  try {
    const parsed = JSON.parse(output);
    const p = parsed.file_path || parsed.path || parsed.output_path || '';
    if (isImagePath(p)) jsonPath = p;
  } catch {}
  const allPaths = jsonPath ? [jsonPath, ...paths.filter(p => p !== jsonPath)] : paths;
  if (allPaths.length === 0) return <DefaultOutput output={output} />;
  return (
    <div className="flex gap-2 flex-wrap">
      {allPaths.slice(0, 4).map((p, i) => (
        <ImagePreview key={i} path={p} />
      ))}
    </div>
  );
}

function WebSearchOutput({ output }) {
  const results = useMemo(() => {
    try {
      const parsed = JSON.parse(output);
      const items = Array.isArray(parsed) ? parsed : parsed.results || parsed.data || [];
      if (Array.isArray(items) && items.length > 0 && (items[0].url || items[0].link)) return items;
    } catch {}
    return null;
  }, [output]);

  if (!results) return <DefaultOutput output={output} />;

  return (
    <div className="space-y-2">
      {results.map((item, i) => (
        <div key={i} className="rounded-lg bg-gray-800/50 p-2">
          <a
            href={item.url || item.link}
            target="_blank"
            rel="noopener noreferrer"
            className="text-sm text-dream-400 hover:text-dream-300 hover:underline font-medium"
          >
            {item.title || item.url || item.link}
          </a>
          {(item.snippet || item.description) && (
            <p className="text-xs text-gray-400 mt-0.5 line-clamp-2">{item.snippet || item.description}</p>
          )}
          <p className="text-xs text-gray-600 mt-0.5 font-mono truncate">{item.url || item.link}</p>
        </div>
      ))}
    </div>
  );
}

function ServiceHealthGrid({ output }) {
  const services = useMemo(() => {
    try {
      const parsed = JSON.parse(output);
      if (Array.isArray(parsed)) return parsed;
      if (parsed.services && Array.isArray(parsed.services)) return parsed.services;
      return Object.entries(parsed).map(([name, val]) => {
        const status = typeof val === 'string' ? val : val?.status || 'unknown';
        return { name, status };
      });
    } catch {}
    return null;
  }, [output]);

  if (!services) return <DefaultOutput output={output} />;

  const dotColor = (status) => {
    const s = (typeof status === 'string' ? status : '').toLowerCase();
    if (['healthy', 'running', 'ok', 'up'].includes(s)) return 'bg-green-500';
    if (['degraded', 'warning', 'slow'].includes(s)) return 'bg-yellow-500';
    if (['down', 'error', 'unhealthy', 'stopped'].includes(s)) return 'bg-red-500';
    return 'bg-gray-500';
  };

  return (
    <div className="grid grid-cols-2 sm:grid-cols-3 gap-1.5">
      {services.map((svc, i) => {
        const name = svc.name || svc.service || svc.id || `service-${i}`;
        const status = svc.status || 'unknown';
        return (
          <div key={i} className="flex items-center gap-1.5 bg-gray-800/60 rounded px-2 py-1">
            <span className={`inline-block w-2 h-2 rounded-full flex-shrink-0 ${dotColor(status)}`} />
            <span className="text-[11px] text-gray-300 truncate">{name}</span>
          </div>
        );
      })}
    </div>
  );
}

function ReadFileOutput({ output, args }) {
  const filePath = extractFilename(args) || extractFilePathFromOutput(output);
  const lineCount = output ? output.split('\n').length : 0;
  return (
    <div>
      <div className="flex items-center gap-2 mb-2">
        <FilenameBadge filePath={filePath} color="purple" />
        <span className="text-xs text-gray-500">{lineCount} lines</span>
      </div>
      <DefaultOutput output={output} />
    </div>
  );
}

function WriteFileOutput({ output, args, toolName }) {
  const filePath = extractFilename(args) || extractFilePathFromOutput(output);
  const color = toolName === 'write_file' || toolName === 'create_file' ? 'green' : 'blue';
  return (
    <div>
      <div className="mb-2">
        <FilenameBadge filePath={filePath} color={color} />
      </div>
      <DefaultOutput output={output} />
    </div>
  );
}

function DefaultOutput({ output }) {
  if (!output) return null;
  return (
    <pre className="text-xs text-gray-300 overflow-x-auto max-h-64 overflow-y-auto whitespace-pre-wrap break-all">
      {output.length > 2000 ? output.slice(0, 2000) + '\n... (truncated)' : output}
    </pre>
  );
}

/* ── Expanded content router ──────────────────────────────────────────── */

function ExpandedContent({ message }) {
  const toolName = (message.toolName || '').toLowerCase();
  const isFileTool = FILE_TOOLS.includes(toolName);
  const isBash = BASH_TOOLS.includes(toolName);

  return (
    <div className="mt-2 pt-2 border-t border-gray-700">
      {/* Args section (shared) */}
      {message.args && (
        <pre className="text-xs text-gray-400 overflow-x-auto mb-2">
          {typeof message.args === 'string' ? message.args : JSON.stringify(message.args, null, 2)}
        </pre>
      )}

      {/* Rich output based on tool type */}
      {message.output && (() => {
        if (isFileTool) {
          return <WriteFileOutput output={message.output} args={message.args} toolName={toolName} />;
        }
        if (isBash) {
          return <BashOutput output={message.output} />;
        }
        if (toolName === 'generateimage') {
          return <GenerateImageOutput output={message.output} />;
        }
        if (toolName === 'websearch') {
          return <WebSearchOutput output={message.output} />;
        }
        if (toolName === 'servicehealth') {
          return <ServiceHealthGrid output={message.output} />;
        }
        if (toolName === 'read_file') {
          return <ReadFileOutput output={message.output} args={message.args} />;
        }
        return <DefaultOutput output={message.output} />;
      })()}
    </div>
  );
}

/* ── Main card ────────────────────────────────────────────────────────── */

export default function ToolCallCard({ message }) {
  const [expanded, setExpanded] = useState(false);
  const isStart = message.type === 'start';
  const isError = message.isError;
  const toolName = message.toolName || '';
  const isFileTool = FILE_TOOLS.includes(toolName);
  const filename = isFileTool ? extractFilename(message.args) : null;

  return (
    <div
      className={`rounded-xl border px-4 py-2 text-sm mx-8 ${
        isError
          ? 'border-red-700 bg-red-900/20'
          : isStart
          ? 'border-yellow-700 bg-yellow-900/20'
          : 'border-green-700 bg-green-900/20'
      }`}
    >
      {/* Collapsed header — tool name + status */}
      <div
        className="flex items-center justify-between cursor-pointer"
        onClick={() => setExpanded(!expanded)}
      >
        <div className="flex items-center gap-2">
          <span className={`text-xs font-mono ${
            isError ? 'text-red-400' : isStart ? 'text-yellow-400' : 'text-green-400'
          }`}>
            {isStart ? '>' : isError ? 'x' : '~'}
          </span>
          <span className="font-mono text-gray-300">{toolName}</span>
          {/* File badge inline in collapsed header */}
          {filename && (
            <span className="px-1.5 py-0.5 rounded bg-blue-900/50 border border-blue-700/50 text-blue-300 text-[10px] font-mono truncate max-w-[200px]">
              {filename.split('/').pop()}
            </span>
          )}
          {isStart && <span className="text-gray-500 text-xs">running...</span>}
          {message.type === 'result' && !isError && (
            <span className="text-green-500 text-xs">done</span>
          )}
          {isError && <span className="text-red-400 text-xs">error</span>}
        </div>
        <span className="text-gray-600 text-xs">{expanded ? 'collapse' : 'expand'}</span>
      </div>

      {/* Rich rendering only when expanded */}
      {expanded && <ExpandedContent message={message} />}
    </div>
  );
}
