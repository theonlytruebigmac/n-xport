import { useRef, useEffect, useState } from 'react';
import type { ProgressUpdate, LogEntry, ImportResult } from '../types';

interface ProgressPanelProps {
    currentStep: 'exporting' | 'complete';
    appMode: 'export' | 'migrate' | 'import';
    progress: ProgressUpdate | null;
    logs: LogEntry[];
    addLog: (level: LogEntry['level'], message: string) => void;
    onOpenOutput: () => void;
    onNewExport: () => void;
    onCancel: () => void;
    /** Last import result, used to render summary chips and the Apply-for-real strip. */
    lastImportResult?: ImportResult | null;
    /** Connected SO display name (for Apply-for-real strip context). */
    targetSoLabel?: string;
    /** Triggers a re-run of the last import config with dryRun=false. */
    onApplyForReal?: () => void;
}

interface ChipDef {
    label: string;
    count: number;
    cls: string;
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
    lastImportResult,
    targetSoLabel,
    onApplyForReal,
}: ProgressPanelProps) {
    const logRef = useRef<HTMLDivElement>(null);
    const [verboseLogging, setVerboseLogging] = useState(false);

    useEffect(() => {
        if (logRef.current) {
            logRef.current.scrollTop = logRef.current.scrollHeight;
        }
    }, [logs]);

    const isImport = appMode === 'import';
    const isComplete = currentStep === 'complete';
    const isDryRun = isImport && lastImportResult?.dryRun === true;
    const showApplyStrip = isComplete && isImport && isDryRun && lastImportResult && (lastImportResult.rowsPlanned > 0);

    const chips: ChipDef[] | null = lastImportResult
        ? [
            isDryRun
                ? { label: 'Would create', count: lastImportResult.rowsPlanned, cls: 'planned' }
                : { label: 'Created', count: lastImportResult.rowsCreated, cls: 'created' },
            { label: isDryRun ? 'Would skip' : 'Skipped', count: lastImportResult.rowsSkipped, cls: 'skipped' },
            { label: isDryRun ? 'Would error' : 'Errored', count: lastImportResult.rowsErrored, cls: 'errored' },
            { label: 'Total rows', count: lastImportResult.rowsTotal, cls: '' },
        ]
        : null;

    return (
        <div className="card fade-in">
            <div className="card-header">
                <h2 className="card-title">
                    {currentStep === 'exporting'
                        ? (appMode === 'migrate' ? 'Migrating…' : isImport ? (isDryRun ? 'Dry-running…' : 'Importing…') : 'Exporting…')
                        : (isImport && isDryRun ? 'Dry-run complete' : 'Complete')}
                </h2>
            </div>

            {/* Summary chips — only for import runs that have completed */}
            {chips && isComplete && (
                <div className="summary-grid">
                    {chips.map(c => (
                        <div key={c.label} className={`stat ${c.cls}`}>
                            <div className="count">{c.count}</div>
                            <div className="label">{c.label}</div>
                        </div>
                    ))}
                </div>
            )}

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
                                try {
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
                                } catch (err) {
                                    console.error('Export failed:', err);
                                    addLog('error', `Failed to export logs: ${err}`);
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

            {/* Apply-for-real confirmation strip (only after a successful dry-run with planned rows) */}
            {showApplyStrip && (
                <div className="confirm-strip">
                    <div className="icon">⚠</div>
                    <div className="text">
                        <strong>{lastImportResult.rowsPlanned} row{lastImportResult.rowsPlanned === 1 ? '' : 's'}</strong> will be created
                        {targetSoLabel && (<> in <strong>{targetSoLabel}</strong></>)}.
                        {lastImportResult.rowsErrored > 0 && (<> {lastImportResult.rowsErrored} row{lastImportResult.rowsErrored === 1 ? '' : 's'} had errors and will be skipped.</>)}
                        {' '}This writes to the live server.
                    </div>
                    <button className="btn-danger" onClick={onApplyForReal}>
                        Apply for real →<span className="kbd">⌘↵</span>
                    </button>
                </div>
            )}

            {currentStep === 'complete' && (
                <div style={{ display: 'flex', gap: 'var(--space-md)', marginTop: 'var(--space-xl)' }}>
                    {appMode === 'export' && (
                        <button className="btn btn-primary btn-lg" style={{ flex: 1 }} onClick={onOpenOutput}>
                            View Export Folder
                        </button>
                    )}
                    <button className="btn btn-secondary btn-lg" style={{ flex: 1 }} onClick={onNewExport}>
                        {appMode === 'migrate' ? 'New Migration' : isImport ? (isDryRun ? 'Edit configuration' : 'Run another import') : 'Start New Export'}
                    </button>
                </div>
            )}
        </div>
    );
}
