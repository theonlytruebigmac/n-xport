import type { ExportType } from '../types';
import { ServiceOrgCombobox } from './ServiceOrgCombobox';
import { MigrationPreflightPanel } from './MigrationPreflightPanel';

interface ConfigurePanelProps {
    appMode: 'export' | 'migrate';
    serviceOrgId: string;
    setServiceOrgId: (v: string) => void;
    outputDir: string;
    setOutputDir: (v: string) => void;
    exportTypes: ExportType[];
    selectedTypes: Set<string>;
    exportFormats: Set<string>;
    onToggleExportType: (id: string) => void;
    /** Replace the whole selection (used by Select all / Clear all). */
    onSetSelectedTypes: (types: Set<string>) => void;
    onToggleFormat: (format: string) => void;
    onBrowseOutput: () => void;
    onBack: () => void;
    /** Optional, used to display the SO name immediately while the discovery list loads. */
    connectedServiceOrgName?: string;
    /** Migration only: source and destination service org IDs, shown in the preflight summary. */
    sourceSoId?: number;
    destSoId?: number;
}

export function ConfigurePanel({
    appMode,
    serviceOrgId, setServiceOrgId,
    outputDir, setOutputDir,
    exportTypes,
    selectedTypes,
    exportFormats,
    onToggleExportType,
    onSetSelectedTypes,
    onToggleFormat,
    onBrowseOutput,
    onBack,
    connectedServiceOrgName,
    sourceSoId,
    destSoId,
}: ConfigurePanelProps) {
    const visibleTypes = exportTypes.filter(type => !(appMode === 'migrate' && type.exportOnly));
    // Types that cannot be picked yet are not part of "Select all"
    const selectableIds = visibleTypes
        .filter(type => !(appMode === 'migrate' && type.migrationComingSoon))
        .map(type => type.id);
    const allSelected = selectableIds.length > 0 && selectableIds.every(id => selectedTypes.has(id));

    return (
        <div className="card card-compact fade-in">
            <div className="grid-2">
                {appMode !== 'migrate' && (
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
                )}
                {appMode !== 'migrate' && (
                    <div className="form-group">
                        <label className="form-label">Output Directory</label>
                        <div style={{ display: 'flex', gap: 'var(--space-sm)' }}>
                            <input type="text" className="form-input" value={outputDir} onChange={e => setOutputDir(e.target.value)} />
                            <button className="btn btn-secondary" onClick={onBrowseOutput}>Browse</button>
                        </div>
                    </div>
                )}
            </div>

            <div className="form-group">
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
                    <label className="form-label" style={{ marginBottom: 0 }}>
                        {appMode === 'migrate' ? 'Data to Migrate' : 'Data to Export'}
                    </label>
                    <button
                        type="button"
                        className="btn btn-link"
                        style={{ marginRight: -6 }}
                        onClick={() => onSetSelectedTypes(allSelected ? new Set() : new Set(selectableIds))}
                    >
                        {allSelected ? 'Clear all' : 'Select all'}
                    </button>
                </div>
                <div className="data-types-grid">
                    {visibleTypes.map(type => {
                        const comingSoon = appMode === 'migrate' && type.migrationComingSoon;
                        return (
                            <label
                                key={type.id}
                                className={`checkbox-item ${selectedTypes.has(type.id) && !comingSoon ? 'selected' : ''} ${comingSoon ? 'disabled' : ''}`}
                                style={comingSoon ? { opacity: 0.5, cursor: 'not-allowed' } : undefined}
                            >
                                <input
                                    type="checkbox"
                                    checked={selectedTypes.has(type.id) && !comingSoon}
                                    disabled={comingSoon}
                                    onChange={() => !comingSoon && onToggleExportType(type.id)}
                                />
                                <span>{type.name}{comingSoon ? <em style={{ marginLeft: '0.4em', fontSize: 'var(--text-xs)', opacity: 0.7 }}>(Coming Soon)</em> : null}</span>
                            </label>
                        );
                    })}
                </div>
            </div>

            {appMode === 'migrate' && (
                <MigrationPreflightPanel
                    selectedTypes={selectedTypes}
                    sourceSoId={sourceSoId}
                    destSoId={destSoId}
                />
            )}

            {appMode !== 'migrate' && (
                <div className="form-group">
                    <label className="form-label">Export Formats</label>
                    <div className="format-grid">
                        {['csv', 'json'].map(f => (
                            <label key={f} className={`checkbox-item ${exportFormats.has(f) ? 'selected' : ''}`}>
                                <input type="checkbox" checked={exportFormats.has(f)} onChange={() => onToggleFormat(f)} />
                                <span style={{ textTransform: 'uppercase' }}>{f}</span>
                            </label>
                        ))}
                    </div>
                </div>
            )}

            <div className="card-actions">
                <span />
                <div style={{ display: 'flex', gap: 8 }}>
                    <button className="btn btn-secondary" onClick={onBack}>Back</button>
                </div>
            </div>
        </div>
    );
}
