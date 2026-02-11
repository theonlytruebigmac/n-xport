import type { Profile, ConnectionStatus, LogEntry } from '../types';
import * as api from '../api';

interface SetupPanelProps {
  appMode: 'export' | 'migrate';
  profiles: Profile[];
  activeProfile: Profile | null;
  // Source connection
  fqdn: string;
  setFqdn: (v: string) => void;
  jwt: string;
  setJwt: (v: string) => void;
  apiUsername: string;
  setApiUsername: (v: string) => void;
  apiPassword: string;
  setApiPassword: (v: string) => void;
  serviceOrgId: string;
  setServiceOrgId: (v: string) => void;
  connectionStatus: ConnectionStatus;
  // Dest connection (migration)
  destFqdn: string;
  setDestFqdn: (v: string) => void;
  destJwt: string;
  setDestJwt: (v: string) => void;
  destApiUsername: string;
  setDestApiUsername: (v: string) => void;
  destServiceOrgId: string;
  setDestServiceOrgId: (v: string) => void;
  destConnectionStatus: ConnectionStatus;
  setDestConnectionStatus: (v: ConnectionStatus) => void;
  setDestServerUrl: (v: string) => void;
  setDestServerVersion: (v: string) => void;
  setDestConnectedServiceOrg: (v: { id: number; name: string } | null) => void;
  // Callbacks
  onConnect: () => void;
  onSelectProfile: (profile: Profile) => void;
  onShowNewProfile: () => void;
  onBack: () => void;
  addLog: (level: LogEntry['level'], message: string) => void;
  loadProfiles: () => Promise<void>;
  setActiveProfile: (profile: Profile | null) => void;
}

export function SetupPanel({
  appMode,
  profiles,
  activeProfile,
  fqdn, setFqdn,
  jwt, setJwt,
  apiUsername, setApiUsername,
  serviceOrgId, setServiceOrgId,
  connectionStatus,
  destFqdn, setDestFqdn,
  destJwt, setDestJwt,
  destApiUsername, setDestApiUsername,
  destServiceOrgId, setDestServiceOrgId,
  destConnectionStatus, setDestConnectionStatus,
  setDestServerUrl, setDestServerVersion,
  setDestConnectedServiceOrg,
  onConnect,
  onSelectProfile,
  onShowNewProfile,
  onBack,
  addLog,
  loadProfiles,
  setActiveProfile,
}: SetupPanelProps) {
  const handleDestConnect = async () => {
    if (!destFqdn || !destJwt || !destApiUsername) {
      addLog('error', 'Please enter all Destination fields (FQDN, JWT, Username)');
      return;
    }
    setDestConnectionStatus('connecting');
    addLog('info', `Connecting to Destination: ${destFqdn}...`);
    try {
      const result = await api.connectDestination(destFqdn, destJwt, destApiUsername);
      if (result.success) {
        setDestConnectionStatus('connected');
        setDestServerUrl(result.serverUrl || destFqdn);
        setDestServerVersion(result.serverVersion || '');

        let finalDestSoId = result.serviceOrgId;
        let finalDestSoName = result.serviceOrgName;

        if (destServiceOrgId) {
          const id = parseInt(destServiceOrgId);
          if (!isNaN(id)) {
            finalDestSoId = id;
            if (finalDestSoId !== result.serviceOrgId) {
              try {
                const info = await api.getServiceOrgInfo(finalDestSoId);
                finalDestSoName = info.name;
              } catch {
                finalDestSoName = `Unknown (ID: ${finalDestSoId})`;
              }
            }
          }
        } else if (result.serviceOrgId) {
          setDestServiceOrgId(result.serviceOrgId.toString());
        }

        if (finalDestSoId && finalDestSoName) {
          setDestConnectedServiceOrg({ id: finalDestSoId, name: finalDestSoName });
          addLog('info', `Destination Service Org: ${finalDestSoName} (ID: ${finalDestSoId})`);
        }

        addLog('success', `Connected to Destination: ${destFqdn}`);
      } else {
        setDestConnectionStatus('error');
        addLog('error', `Destination Error: ${result.message}`);
      }
    } catch (e) {
      setDestConnectionStatus('error');
      addLog('error', `Destination Connection failed: ${e}`);
    }
  };

  return (
    <div className="centered-dashboard fade-in">
      {/* Back to Home / Mode Indicator */}
      <div className="card" style={{ padding: 'var(--space-sm)', marginBottom: 'var(--space-md)' }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
          <button
            className="btn btn-ghost"
            onClick={onBack}
            style={{ display: 'flex', alignItems: 'center', gap: 'var(--space-xs)' }}
          >
            ← Back
          </button>
          <span style={{ color: 'var(--color-text)', fontWeight: 600 }}>
            {appMode === 'export' ? 'Export Mode' : 'Migration Mode'}
          </span>
          <div style={{ width: 80 }} />
        </div>
      </div>

      <div>
        {/* Profile Selection */}
        <div className="card card-compact fade-in" style={{ marginBottom: 'var(--space-md)' }}>
          <div className="card-header">
            <h2 className="card-title">Select <span className="header-accent">Profile</span></h2>
          </div>
          <div className="profiles-grid">
            {profiles.map(profile => (
              <div
                key={profile.name}
                className={`profile-card ${activeProfile?.name === profile.name ? 'active' : ''}`}
                onClick={() => onSelectProfile(profile)}
              >
                <div className="profile-info">
                  <span className="profile-name">{profile.name}</span>
                  <span className="profile-fqdn">{profile.source.fqdn}</span>
                </div>
                <button
                  className="profile-delete"
                  onClick={async (e) => {
                    e.stopPropagation();
                    if (confirm(`Delete profile "${profile.name}"?`)) {
                      try {
                        await api.deleteProfile(profile.name);
                        await api.deleteCredentials(profile.name);
                        addLog('info', `Deleted profile "${profile.name}"`);
                        await loadProfiles();
                        if (activeProfile?.name === profile.name) {
                          setActiveProfile(null);
                          setFqdn('');
                          setServiceOrgId('');
                        }
                      } catch (err) {
                        addLog('error', `Failed to delete: ${err}`);
                      }
                    }
                  }}
                >
                  ×
                </button>
              </div>
            ))}
            <button className="profile-card new" onClick={onShowNewProfile}>
              <span>+ New Profile</span>
            </button>
          </div>
        </div>
      </div>

      <div className={appMode === 'migrate' ? 'setup-grid' : ''}>
        {/* Source Connection */}
        <div className="card card-compact fade-in">
          <div className="card-header">
            <h2 className="card-title">
              {appMode === 'migrate' ? 'Source' : 'Direct'} <span className="header-accent">Connection</span>
            </h2>
          </div>
          <div className="form-group">
            <label className="form-label">Server FQDN</label>
            <input type="text" className="form-input" placeholder="ncentral.example.com" value={fqdn} onChange={e => setFqdn(e.target.value)} />
          </div>
          <div className="form-group">
            <label className="form-label">API Username <span className="text-secondary" style={{ fontSize: '0.7em' }}>(Required for User Add)</span></label>
            <input type="text" className="form-input" placeholder="admin@example.com" value={apiUsername} onChange={e => setApiUsername(e.target.value)} />
          </div>
          <div className="form-group">
            <label className="form-label">JWT Token</label>
            <input type="password" className="form-input mono" placeholder="eyJhbGciOiJIUzI1NiIs..." value={jwt} onChange={e => setJwt(e.target.value)} />
          </div>
          <div className="form-group">
            <label className="form-label">Target Service Org ID</label>
            <input type="number" className="form-input" placeholder="Service Org ID (optional)" value={serviceOrgId} onChange={e => setServiceOrgId(e.target.value)} />
          </div>
          <button
            className="btn btn-primary btn-lg"
            style={{ width: '100%' }}
            onClick={onConnect}
            disabled={connectionStatus === 'connecting'}
          >
            {connectionStatus === 'connecting' ? 'Connecting...' : appMode === 'migrate' ? 'Connect Source' : 'Connect Now'}
          </button>
        </div>

        {/* Destination Connection (Migration only) */}
        {appMode === 'migrate' && (
          <div className="card card-compact fade-in">
            <div className="card-header">
              <h2 className="card-title">Destination <span className="header-accent">Connection</span></h2>
            </div>
            <div className="form-group">
              <label className="form-label">Server FQDN</label>
              <input type="text" className="form-input" placeholder="destination.example.com" value={destFqdn} onChange={e => setDestFqdn(e.target.value)} />
            </div>
            <div className="form-group">
              <label className="form-label">API Username <span className="text-secondary" style={{ fontSize: '0.7em' }}>(Required for User Add)</span></label>
              <input type="text" className="form-input" placeholder="admin@example.com" value={destApiUsername} onChange={e => setDestApiUsername(e.target.value)} />
            </div>
            <div className="form-group">
              <label className="form-label">JWT Token</label>
              <input type="password" className="form-input mono" placeholder="eyJhbGciOiJIUzI1NiIs..." value={destJwt} onChange={e => setDestJwt(e.target.value)} />
            </div>
            <div className="form-group">
              <label className="form-label">Target Service Org ID</label>
              <input type="number" className="form-input" placeholder="Service Org ID (optional)" value={destServiceOrgId} onChange={e => setDestServiceOrgId(e.target.value)} />
            </div>
            <button
              className="btn btn-primary btn-lg"
              style={{ width: '100%' }}
              onClick={handleDestConnect}
              disabled={destConnectionStatus === 'connecting'}
            >
              {destConnectionStatus === 'connecting' ? 'Connecting...' : 'Connect Destination'}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
