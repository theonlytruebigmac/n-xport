interface HomePanelProps {
    appVersion: string;
    onSelectExport: () => void;
    onSelectMigrate: () => void;
}

export function HomePanel({ appVersion, onSelectExport, onSelectMigrate }: HomePanelProps) {
    return (
        <div className="centered-dashboard fade-in" style={{
            maxWidth: 750,
            margin: '0 auto',
            paddingTop: 'var(--space-lg)',
            display: 'flex',
            flexDirection: 'column',
            height: 'calc(100vh - 120px)'
        }}>
            <div style={{ textAlign: 'center', marginBottom: 'var(--space-xl)' }}>
                <h1 style={{
                    fontSize: '2rem',
                    fontWeight: 600,
                    marginBottom: 'var(--space-xs)',
                    color: 'var(--color-text)'
                }}>
                    <span style={{ color: 'var(--color-accent)' }}>N-xport</span> Data Tool
                </h1>
                <p style={{ color: 'var(--color-text-muted)', fontSize: '0.9375rem' }}>
                    Export or migrate data between N-Central servers
                </p>
            </div>

            <div style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(2, 1fr)',
                gap: 'var(--space-lg)',
                flex: 1,
                minHeight: 280
            }}>
                {/* Export Card */}
                <ModeCard
                    title="Export Data"
                    description="Export customers, sites, users, and devices to CSV or JSON"
                    accentColor="var(--color-accent)"
                    onClick={onSelectExport}
                />

                {/* Migration Card */}
                <ModeCard
                    title="Migrate Data"
                    description="Transfer customers, users, roles, and properties between servers"
                    accentColor="var(--color-success)"
                    onClick={onSelectMigrate}
                />
            </div>

            <div style={{
                textAlign: 'center',
                marginTop: 'var(--space-lg)',
                color: 'var(--color-text-muted)',
                fontSize: '0.75rem',
                opacity: 0.5
            }}>
                v{appVersion}
            </div>
        </div>
    );
}

function ModeCard({ title, description, accentColor, onClick }: {
    title: string;
    description: string;
    accentColor: string;
    onClick: () => void;
}) {
    return (
        <div
            className="card"
            onClick={onClick}
            style={{
                cursor: 'pointer',
                padding: 0,
                overflow: 'hidden',
                transition: 'all 0.2s ease',
                border: '1px solid var(--color-border)',
                display: 'flex',
                flexDirection: 'column'
            }}
            onMouseEnter={(e) => {
                e.currentTarget.style.borderColor = accentColor;
                e.currentTarget.style.transform = 'translateY(-2px)';
            }}
            onMouseLeave={(e) => {
                e.currentTarget.style.borderColor = 'var(--color-border)';
                e.currentTarget.style.transform = 'translateY(0)';
            }}
        >
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
                    fontSize: '1.5rem',
                    fontWeight: 600,
                    marginBottom: 'var(--space-sm)',
                    color: 'var(--color-text)'
                }}>{title}</h2>
                <p style={{
                    color: 'var(--color-text-muted)',
                    fontSize: '0.875rem',
                    lineHeight: 1.5,
                    maxWidth: 220
                }}>
                    {description}
                </p>
            </div>
        </div>
    );
}
