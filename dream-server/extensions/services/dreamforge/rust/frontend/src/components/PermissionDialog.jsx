import React from 'react';

export default function PermissionDialog({ permission, onRespond }) {
  if (!permission) return null;

  return (
    <div className="mx-8 rounded-xl border border-amber-600 bg-amber-900/30 p-4">
      <div className="flex items-start gap-3">
        <span className="text-amber-400 text-xl mt-0.5">!</span>
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium text-amber-200 mb-1">Permission Required</p>
          <p className="text-sm text-gray-300 mb-2">{permission.message || `Allow ${permission.tool_name}?`}</p>
          {permission.arguments && (
            <pre className="text-xs text-gray-400 bg-gray-900 rounded p-2 mb-3 overflow-x-auto max-h-32 overflow-y-auto">
              {typeof permission.arguments === 'string'
                ? permission.arguments
                : JSON.stringify(permission.arguments, null, 2)}
            </pre>
          )}
          <div className="flex gap-2">
            <button
              onClick={() => onRespond(permission.request_id, true)}
              className="px-4 py-1.5 rounded-lg bg-green-700 hover:bg-green-600 text-white text-sm font-medium transition-colors"
            >
              Allow
            </button>
            <button
              onClick={() => onRespond(permission.request_id, false)}
              className="px-4 py-1.5 rounded-lg bg-red-700 hover:bg-red-600 text-white text-sm font-medium transition-colors"
            >
              Deny
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
