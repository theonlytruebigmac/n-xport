interface NewProfileModalProps {
    appMode: 'export' | 'migrate';
    newProfileName: string;
    setNewProfileName: (v: string) => void;
    fqdn: string;
    setFqdn: (v: string) => void;
    destFqdn: string;
    connectedServiceOrg: { id: number; name: string } | null;
    destConnectedServiceOrg: { id: number; name: string } | null;
    onSave: () => void;
    onClose: () => void;
}

export function NewProfileModal({
    appMode,
    newProfileName,
    setNewProfileName,
    fqdn,
    setFqdn,
    destFqdn,
    connectedServiceOrg,
    destConnectedServiceOrg,
    onSave,
    onClose,
}: NewProfileModalProps) {
    return (
        <div className="modal-overlay" onClick={onClose}>
            <div className="card modal-content" onClick={e => e.stopPropagation()}>
                <div className="card-header">
                    <h2 className="card-title">
                        {appMode === 'migrate' ? 'Save Migration Profile' : 'Create New Profile'}
                    </h2>
                </div>
                <div className="form-group">
                    <label className="form-label">Profile Name</label>
                    <input
                        type="text"
                        className="form-input"
                        placeholder={appMode === 'migrate' ? 'My Migration' : 'My Server'}
                        value={newProfileName}
                        onChange={e => setNewProfileName(e.target.value)}
                    />
                </div>

                {appMode === 'migrate' ? (
                    <>
                        <div className="form-group" style={{ opacity: 0.8 }}>
                            <label className="form-label">Source</label>
                            <div style={{ padding: 'var(--space-sm)', background: 'var(--color-bg-tertiary)', borderRadius: 'var(--radius-sm)', fontSize: '0.875rem' }}>
                                {fqdn || <span style={{ color: 'var(--color-text-secondary)' }}>Not configured</span>}
                                {connectedServiceOrg && <span style={{ marginLeft: 'var(--space-sm)', color: 'var(--color-text-secondary)' }}>• {connectedServiceOrg.name}</span>}
                            </div>
                        </div>
                        <div className="form-group" style={{ opacity: 0.8 }}>
                            <label className="form-label">Destination</label>
                            <div style={{ padding: 'var(--space-sm)', background: 'var(--color-bg-tertiary)', borderRadius: 'var(--radius-sm)', fontSize: '0.875rem' }}>
                                {destFqdn || <span style={{ color: 'var(--color-text-secondary)' }}>Not configured</span>}
                                {destConnectedServiceOrg && <span style={{ marginLeft: 'var(--space-sm)', color: 'var(--color-text-secondary)' }}>• {destConnectedServiceOrg.name}</span>}
                            </div>
                        </div>
                    </>
                ) : (
                    <div className="form-group">
                        <label className="form-label">Server FQDN</label>
                        <input type="text" className="form-input" placeholder="ncentral.example.com" value={fqdn} onChange={e => setFqdn(e.target.value)} />
                    </div>
                )}

                <div style={{ display: 'flex', gap: 'var(--space-md)' }}>
                    <button
                        className="btn btn-primary"
                        onClick={onSave}
                        disabled={!newProfileName || !fqdn || (appMode === 'migrate' && !destFqdn)}
                    >
                        Save Profile
                    </button>
                    <button className="btn btn-ghost" onClick={onClose}>Cancel</button>
                </div>
            </div>
        </div>
    );
}
