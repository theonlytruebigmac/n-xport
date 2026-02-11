import { useRef, useEffect, useState } from 'react';
import type { ProgressUpdate, LogEntry } from '../types';

interface ProgressPanelProps {
    currentStep: 'exporting' | 'complete';
    appMode: 'export' | 'migrate';
    progress: ProgressUpdate | null;
    logs: LogEntry[];
    addLog: (level: LogEntry['level'], message: string) => void;
    onOpenOutput: () => void;
    onNewExport: () => void;
    onCancel: () => void;
}

export function ProgressPanel({
    currentStep,
    appMode,
    progress,
    logs,
    addLog,
    onOpenOutput,
    onNewExport,
    onCancel,
}: ProgressPanelProps) {
    const logRef = useRef<HTMLDivElement>(null);
    const [verboseLogging, setVerboseLogging] = useState(false);

    useEffect(() => {
        if (logRef.current) {
            logRef.current.scrollTop = logRef.current.scrollHeight;
        }
    }, [logs]);

    return (
        <div className="card fade-in">
            <div className="card-header">
                <h2 className="card-title">
                    {currentStep === 'exporting' ? (appMode === 'migrate' ? 'Migrating...' : 'Exporting...') : 'Complete'}
                </h2>
            </div>

            {progress && (
                <div className="progress-container large">
                    <div className="progress-bar">
                        <div className="progress-fill" style={{ width: `${progress.percent}%` }} />
                    </div>
                    <div className="progress-stats">
                        <span className="phase">{progress.phase}</span>
                        <span className="percent">{Math.round(progress.percent)}%</span>
                    </div>
                    <div className="progress-message">{progress.message}</div>
                    {currentStep === 'exporting' && (
                        <button
                            className="btn btn-secondary"
                            style={{ marginTop: 'var(--space-sm)', color: 'var(--color-error)', borderColor: 'var(--color-error)' }}
                            onClick={onCancel}
                        >
                            Cancel
                        </button>
                    )}
                </div>
            )}

            <div className="live-logs">
                <div className="logs-header" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                    <span>Activity Log</span>
                    <div style={{ display: 'flex', gap: 'var(--space-sm)', alignItems: 'center' }}>
                        <label style={{ display: 'flex', alignItems: 'center', gap: 'var(--space-xs)', fontSize: '0.75rem', cursor: 'pointer' }}>
                            <input
                                type="checkbox"
                                checked={verboseLogging}
                                onChange={(e) => setVerboseLogging(e.target.checked)}
                                style={{ width: '14px', height: '14px' }}
                            />
                            Verbose
                        </label>
                        <button
                            className="btn btn-ghost"
                            style={{ padding: '4px 8px', fontSize: '0.7rem' }}
                            onClick={async () => {
                                const { save } = await import('@tauri-apps/plugin-dialog');
                                const { writeTextFile } = await import('@tauri-apps/plugin-fs');
                                const filePath = await save({
                                    title: 'Export Logs',
                                    defaultPath: `nc-export-logs-${new Date().toISOString().slice(0, 10)}.txt`,
                                    filters: [{ name: 'Text Files', extensions: ['txt'] }]
                                });
                                if (filePath) {
                                    const logText = logs.map(l =>
                                        `[${l.timestamp.toLocaleString()}] [${l.level.toUpperCase()}] ${l.message}`
                                    ).join('\n');
                                    await writeTextFile(filePath, logText);
                                    addLog('success', `Logs exported to ${filePath}`);
                                }
                            }}
                        >
                            Export
                        </button>
                    </div>
                </div>
                <div className="log-panel" ref={logRef}>
                    {logs
                        .filter(log => verboseLogging || log.level !== 'debug')
                        .slice(-500)
                        .map((log, i) => (
                            <div key={i} className={`log-entry ${log.level}`}>
                                <span className="log-time">[{log.timestamp.toLocaleTimeString()}]</span> {log.message}
                            </div>
                        ))}
                </div>
            </div>

            {currentStep === 'complete' && (
                <div style={{ display: 'flex', gap: 'var(--space-md)', marginTop: 'var(--space-xl)' }}>
                    {appMode !== 'migrate' && (
                        <button className="btn btn-primary btn-lg" style={{ flex: 1 }} onClick={onOpenOutput}>
                            View Export Folder
                        </button>
                    )}
                    <button className="btn btn-secondary btn-lg" style={{ flex: 1 }} onClick={onNewExport}>
                        {appMode === 'migrate' ? 'New Migration' : 'Start New Export'}
                    </button>
                </div>
            )}
        </div>
    );
}
