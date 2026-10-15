interface HomePanelProps {
    appVersion: string;
    onSelectExport: () => void;
    onSelectMigrate: () => void;
    onSelectImport: () => void;
    onOpenSettings: () => void;
    onSelectInstallers: () => void;
}

export function HomePanel({ appVersion, onSelectExport, onSelectMigrate, onSelectImport, onOpenSettings, onSelectInstallers }: HomePanelProps) {
    return (
        <div className="centered-dashboard fade-in" style={{
            maxWidth: 1000,
            margin: '0 auto',
            paddingTop: 'var(--space-lg)',
            display: 'flex',
            flexDirection: 'column',
            justifyContent: 'center',
            height: 'calc(100vh - 120px)'
        }}>
            <button
                className="settings-button"
                onClick={onOpenSettings}
                aria-label="Settings"
                title="Settings"
            >
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                    strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <circle cx="12" cy="12" r="3" />
                    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
                </svg>
            </button>
            <div style={{ textAlign: 'center', marginBottom: 'var(--space-xl)' }}>
                <h1 style={{
                    fontSize: 'var(--text-hero)',
                    fontWeight: 600,
                    marginBottom: 'var(--space-xs)',
                    color: 'var(--color-text)'
                }}>
                    <span style={{ color: 'var(--color-accent)' }}>N-xport</span> Data Tool
                </h1>
                <p style={{ color: 'var(--color-text-muted)', fontSize: 'var(--text-base)' }}>
                    Export, import, or migrate data between N-Central servers
                </p>
            </div>

            <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(3, 1fr)',
                gap: 'var(--space-lg)',
                flex: 1,
                minHeight: 200,
                // In a tall window the cards stop growing, so they never become empty slabs
                maxHeight: 420
            }}>
                {/* Export Card — recommended */}
                <ModeCard
                    title="Export Data"
                    description="Pull customers, sites, users, and devices into CSV or JSON."
                    accentColor="var(--color-accent)"
                    onClick={onSelectExport}
                    recommended
                />

                {/* Import Card */}
                <ModeCard
                    title="Import Data"
                    description="Load customers, sites, access groups, roles, and users from a CSV."
                    accentColor="var(--color-warning, #fbbf24)"
                    onClick={onSelectImport}
                />

                {/* Migration Card */}
                <ModeCard
                    title="Migrate Data"
                    description="Copy customers, users, roles, and properties between two N-central servers."
                    accentColor="var(--color-success)"
                    onClick={onSelectMigrate}
                />
            </div>

            <div
                className="card mode-card other-tools"
                role="button"
                tabIndex={0}
                onClick={onSelectInstallers}
                onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelectInstallers(); } }}
            >
                <span className="other-tools-title">Generate Installer Scripts</span>
                <span className="other-tools-text">Create agent install scripts for customers and sites</span>
            </div>

            <div style={{
                textAlign: 'center',
                marginTop: 'var(--space-lg)',
                color: 'var(--color-text-muted)',
                fontSize: 'var(--text-xs)',
                opacity: 0.5
            }}>
                v{appVersion}
            </div>
        </div>
    );
}

function ModeCard({ title, description, accentColor, onClick, recommended }: {
    title: string;
    description: string;
    accentColor: string;
    onClick: () => void;
    recommended?: boolean;
}) {
    return (
        <div
            className={`card mode-card${recommended ? ' recommended' : ''}`}
            onClick={onClick}
            style={{
                cursor: 'pointer',
                padding: 0,
                overflow: 'hidden',
                border: '1px solid var(--color-border)',
                display: 'flex',
                flexDirection: 'column',
                position: 'relative',
                ['--mode-accent' as string]: accentColor,
            }}
        >
            {recommended && <span className="recommended-pill">Most common</span>}
            <div style={{ height: 4, background: accentColor }} />
            <div style={{
                padding: 'var(--space-xl)',
                flex: 1,
                display: 'flex',
                flexDirection: 'column',
                justifyContent: 'center',
                alignItems: 'center',
                textAlign: 'center'
            }}>
                <h2 style={{
                    fontSize: 'var(--text-lg)',
                    fontWeight: 600,
                    marginBottom: 'var(--space-sm)',
                    color: 'var(--color-text)'
                }}>{title}</h2>
                <p style={{
                    color: 'var(--color-text-muted)',
                    fontSize: 'var(--text-base)',
                    lineHeight: 1.5,
                    maxWidth: 240
                }}>
                    {description}
                </p>
            </div>
        </div>
    );
}
