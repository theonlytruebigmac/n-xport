import { useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import * as api from '../api';
import type { ImportType, LogEntry, PasswordPolicy } from '../types';
import { DEFAULT_PASSWORD_POLICY } from '../types';
import { ServiceOrgCombobox } from './ServiceOrgCombobox';
import { PropertyPrerequisitesNotice } from './PropertyPrerequisitesNotice';
import { SoapApiNotice } from './SoapApiNotice';
import { ImportTemplatesMenu } from './ImportTemplatesMenu';

interface ImportPanelProps {
    serviceOrgId: string;
    setServiceOrgId: (v: string) => void;
    csvPath: string;
    setCsvPath: (v: string) => void;
    selectedResource: string;
    setSelectedResource: (id: string) => void;
    dryRun: boolean;
    setDryRun: (v: boolean) => void;
    passwordPolicy: PasswordPolicy;
    setPasswordPolicy: (p: PasswordPolicy) => void;
    onBack: () => void;
    addLog: (level: LogEntry['level'], message: string) => void;
    /** Optional, used to display the SO name immediately while the discovery list loads. */
    connectedServiceOrgName?: string;
}

const POLICY_FIELDS: Array<{ key: keyof PasswordPolicy; label: string; min: number; max: number }> = [
    { key: 'minLength', label: 'Minimum password length', min: 1, max: 128 },
    { key: 'minSpecial', label: 'Minimum special characters', min: 0, max: 32 },
    { key: 'minDigits', label: 'Minimum digits', min: 0, max: 32 },
    { key: 'minUppercase', label: 'Minimum uppercase characters', min: 0, max: 32 },
    { key: 'minLowercase', label: 'Minimum lowercase characters', min: 0, max: 32 },
];

export function ImportPanel({
    serviceOrgId,
    setServiceOrgId,
    csvPath,
    setCsvPath,
    selectedResource,
    setSelectedResource,
    dryRun,
    setDryRun,
    passwordPolicy,
    setPasswordPolicy,
    onBack,
    addLog,
    connectedServiceOrgName,
}: ImportPanelProps) {
    const [importTypes, setImportTypes] = useState<ImportType[]>([]);

    useEffect(() => {
        api.getImportTypes()
            .then(setImportTypes)
            .catch((e) => addLog('error', `Failed to load import types: ${e}`));
    }, [addLog]);

    const handleBrowseCsv = async () => {
        const selected = await open({
            title: 'Select CSV file to import',
            filters: [{ name: 'CSV', extensions: ['csv'] }],
            multiple: false,
            directory: false,
        });
        if (typeof selected === 'string') setCsvPath(selected);
    };

    const supportedTypes = importTypes.filter(t => t.supported);
    const unsupportedTypes = importTypes.filter(t => !t.supported);
    const fileLabel = csvPath ? csvPath.split(/[\\/]/).pop() : null;

    return (
        <div className="card card-compact fade-in">
            <div className="grid-2">
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
            </div>

            <div className="form-group">
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
                    <label className="form-label" style={{ marginBottom: 0 }}>Resource to Import</label>
                    <ImportTemplatesMenu addLog={addLog} />
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
                        <span className="head">Not yet supported for import:</span>{' '}
                        <span className="items">
                            {unsupportedTypes.map(t => t.name).join(', ')}
                        </span>
                    </div>
                )}
            </div>

            {['customers', 'access_groups', 'user_roles', 'users'].includes(selectedResource)
                && <SoapApiNotice />}

            {(selectedResource === 'org_properties' || selectedResource === 'device_properties')
                && <PropertyPrerequisitesNotice resource={selectedResource} />}

            {selectedResource === 'users' && (
                <div className="form-group password-policy">
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
                        <label className="form-label" style={{ marginBottom: 0 }}>Password Complexity</label>
                        <button
                            type="button"
                            className="btn btn-link"
                            onClick={() => setPasswordPolicy(DEFAULT_PASSWORD_POLICY)}
                            title="Reset to N-central defaults"
                        >
                            Reset
                        </button>
                    </div>
                    <p className="form-help" style={{ marginTop: 0, marginBottom: 12 }}>
                        Match the target server's policy (System &gt; Administration &gt; User Management &gt; Password Settings). Generated passwords will satisfy all minimums below.
                    </p>
                    <div className="policy-grid">
                        {POLICY_FIELDS.map(({ key, label, min, max }) => (
                            <div key={key} className="policy-field">
                                <label className="form-label form-label-sm">{label}</label>
                                <input
                                    type="number"
                                    className="form-input"
                                    min={min}
                                    max={max}
                                    value={passwordPolicy[key]}
                                    onChange={(e) => {
                                        const raw = parseInt(e.target.value, 10);
                                        const v = Number.isFinite(raw) ? Math.max(min, Math.min(max, raw)) : min;
                                        setPasswordPolicy({ ...passwordPolicy, [key]: v });
                                    }}
                                />
                            </div>
                        ))}
                    </div>
                </div>
            )}

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
