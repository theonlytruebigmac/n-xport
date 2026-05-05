import { useEffect, useRef, useState } from 'react';
import { open, save } from '@tauri-apps/plugin-dialog';
import * as api from '../api';
import type { ImportType, LogEntry } from '../types';
import { ServiceOrgCombobox } from './ServiceOrgCombobox';

interface ImportPanelProps {
    serviceOrgId: string;
    setServiceOrgId: (v: string) => void;
    csvPath: string;
    setCsvPath: (v: string) => void;
    selectedResource: string;
    setSelectedResource: (id: string) => void;
    dryRun: boolean;
    setDryRun: (v: boolean) => void;
    onBack: () => void;
    addLog: (level: LogEntry['level'], message: string) => void;
    /** Optional, used to display the SO name immediately while the discovery list loads. */
    connectedServiceOrgName?: string;
}

export function ImportPanel({
    serviceOrgId,
    setServiceOrgId,
    csvPath,
    setCsvPath,
    selectedResource,
    setSelectedResource,
    dryRun,
    setDryRun,
    onBack,
    addLog,
    connectedServiceOrgName,
}: ImportPanelProps) {
    const [importTypes, setImportTypes] = useState<ImportType[]>([]);
    const [templatesOpen, setTemplatesOpen] = useState(false);
    const templatesMenuRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        api.getImportTypes()
            .then(setImportTypes)
            .catch((e) => addLog('error', `Failed to load import types: ${e}`));
    }, [addLog]);

    useEffect(() => {
        if (!templatesOpen) return;
        const onDocClick = (e: MouseEvent) => {
            if (templatesMenuRef.current && !templatesMenuRef.current.contains(e.target as Node)) {
                setTemplatesOpen(false);
            }
        };
        document.addEventListener('mousedown', onDocClick);
        return () => document.removeEventListener('mousedown', onDocClick);
    }, [templatesOpen]);

    const handleBrowseCsv = async () => {
        const selected = await open({
            title: 'Select CSV file to import',
            filters: [{ name: 'CSV', extensions: ['csv'] }],
            multiple: false,
            directory: false,
        });
        if (typeof selected === 'string') setCsvPath(selected);
    };

    const handleDownloadTemplate = async (resourceId: string, resourceName: string) => {
        setTemplatesOpen(false);
        try {
            const path = await save({
                title: `Save ${resourceName} template`,
                defaultPath: `${resourceId}_template.csv`,
                filters: [{ name: 'CSV', extensions: ['csv'] }],
            });
            if (!path) return;
            const written = await api.generateTemplate(resourceId, path);
            addLog('success', `Template saved to ${written}`);
        } catch (e) {
            addLog('error', `Failed to save template: ${e}`);
        }
    };

    const supportedTypes = importTypes.filter(t => t.supported);
    const unsupportedTypes = importTypes.filter(t => !t.supported);
    const fileLabel = csvPath ? csvPath.split(/[\\/]/).pop() : null;

    return (
        <div className="card card-compact fade-in">
            <div className="card-header">
                <h2 className="card-title">Configure <span className="header-accent">Import</span></h2>
            </div>

            <div className="grid-2">
                <div className="form-group">
                    <label className="form-label">CSV File</label>
                    <div style={{ display: 'flex', gap: 'var(--space-sm)' }}>
                        <input
                            type="text"
                            className="form-input"
                            value={csvPath}
                            placeholder="Choose a CSV file…"
                            onChange={(e) => setCsvPath(e.target.value)}
                        />
                        <button className="btn btn-secondary" onClick={handleBrowseCsv}>Browse</button>
                    </div>
                </div>
                <div className="form-group">
                    <label className="form-label">Target Service Org</label>
                    <ServiceOrgCombobox
                        value={serviceOrgId}
                        onChange={setServiceOrgId}
                        enabled={true}
                        initialName={connectedServiceOrgName}
                        placeholder="Select or type a service org…"
                    />
                </div>
            </div>

            <div className="form-group">
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
                    <label className="form-label" style={{ marginBottom: 0 }}>Resource to Import</label>
                    <div className="templates-menu" ref={templatesMenuRef}>
                        <button className="templates-trigger" onClick={() => setTemplatesOpen(o => !o)}>
                            <span>↓</span> CSV templates <span className="caret">▾</span>
                        </button>
                        {templatesOpen && (
                            <div className="templates-pop">
                                <div className="templates-pop-head">Download a starting CSV</div>
                                {supportedTypes.map(t => (
                                    <button
                                        key={t.id}
                                        className="templates-pop-item"
                                        onClick={() => handleDownloadTemplate(t.id, t.name)}
                                    >
                                        <span>{t.name}</span>
                                        <span className="arrow">↓</span>
                                    </button>
                                ))}
                            </div>
                        )}
                    </div>
                </div>

                <div className="data-types-grid">
                    {supportedTypes.map((t) => {
                        const selected = selectedResource === t.id;
                        return (
                            <div
                                key={t.id}
                                className={`resource-card ${selected ? 'selected' : ''}`}
                                onClick={() => setSelectedResource(t.id)}
                            >
                                <div className="radio-mark" />
                                <span className="name">{t.name}</span>
                            </div>
                        );
                    })}
                </div>

                {unsupportedTypes.length > 0 && (
                    <div className="unsupported-list">
                        <div className="head">Not yet supported for import</div>
                        <div className="items">
                            {unsupportedTypes.map(t => (
                                <span key={t.id}>· {t.name}</span>
                            ))}
                        </div>
                    </div>
                )}
            </div>

            <div className="card-actions">
                <div style={{ display: 'flex', gap: 12, alignItems: 'center' }}>
                    <button
                        type="button"
                        className={`pill-toggle ${dryRun ? 'on' : 'off'}`}
                        onClick={() => setDryRun(!dryRun)}
                        title={dryRun ? 'Validate without writing to N-central' : 'Click to enable dry-run'}
                    >
                        <span className="icon">!</span>
                        Dry-run
                        <span className="switch" />
                    </button>
                    {fileLabel && (
                        <span className="meta">
                            <strong style={{ color: 'var(--color-text-secondary)' }}>{fileLabel}</strong>
                        </span>
                    )}
                </div>
                <div style={{ display: 'flex', gap: 8 }}>
                    <button className="btn btn-secondary" onClick={onBack}>Back</button>
                </div>
            </div>
        </div>
    );
}
