import type { ExportType } from '../types';

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
    onToggleFormat: (format: string) => void;
    onBrowseOutput: () => void;
    onBack: () => void;
}

export function ConfigurePanel({
    appMode,
    serviceOrgId, setServiceOrgId,
    outputDir, setOutputDir,
    exportTypes,
    selectedTypes,
    exportFormats,
    onToggleExportType,
    onToggleFormat,
    onBrowseOutput,
    onBack,
}: ConfigurePanelProps) {
    return (
        <div className="card card-compact fade-in">
            <div className="card-header">
                <h2 className="card-title">Configure <span className="header-accent">{appMode === 'migrate' ? 'Migration' : 'Export'}</span></h2>
            </div>

            <div className="grid-2">
                {appMode !== 'migrate' && (
                    <div className="form-group">
                        <label className="form-label">Target Service Org ID</label>
                        <input type="number" className="form-input" value={serviceOrgId} onChange={e => setServiceOrgId(e.target.value)} />
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
                <label className="form-label">
                    {appMode === 'migrate' ? 'Data to Migrate' : 'Data to Export'}
                </label>
                <div className="data-types-grid">
                    {exportTypes.map(type => (
                        <label key={type.id} className={`checkbox-item ${selectedTypes.has(type.id) ? 'selected' : ''}`}>
                            <input type="checkbox" checked={selectedTypes.has(type.id)} onChange={() => onToggleExportType(type.id)} />
                            <span>{type.name}</span>
                        </label>
                    ))}
                </div>
            </div>

            {appMode !== 'migrate' && (
                <div className="form-group">
                    <label className="form-label">Export Formats</label>
                    <div style={{ display: 'flex', gap: 'var(--space-md)' }}>
                        {['csv', 'json'].map(f => (
                            <label key={f} className={`checkbox-item ${exportFormats.has(f) ? 'selected' : ''}`}>
                                <input type="checkbox" checked={exportFormats.has(f)} onChange={() => onToggleFormat(f)} />
                                <span style={{ textTransform: 'uppercase' }}>{f}</span>
                            </label>
                        ))}
                    </div>
                </div>
            )}

            <div style={{ display: 'flex', gap: 'var(--space-md)', marginTop: 'var(--space-md)' }}>
                <button className="btn btn-secondary btn-lg" style={{ flex: 1 }} onClick={onBack}>
                    Back to Setup
                </button>
            </div>
        </div>
    );
}
