import type { Profile, ConnectionStatus, LogEntry } from '../types';
import * as api from '../api';
import { ServiceOrgCombobox } from './ServiceOrgCombobox';

interface SetupPanelProps {
  appMode: 'export' | 'migrate' | 'import';
  profiles: Profile[];
  activeProfile: Profile | null;
  fqdn: string;
  setFqdn: (v: string) => void;
  jwt: string;
  setJwt: (v: string) => void;
  apiUsername: string;
  setApiUsername: (v: string) => void;
  serviceOrgId: string;
  setServiceOrgId: (v: string) => void;
  connectionStatus: ConnectionStatus;
  /** Source SO discovered from the connection result, used as the combobox's initial display name. */
  connectedServiceOrg?: { id: number; name: string } | null;
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
  destConnectedServiceOrg?: { id: number; name: string } | null;
  onConnect: () => void;
  onSelectProfile: (profile: Profile) => void;
  onShowNewProfile: () => void;
  addLog: (level: LogEntry['level'], message: string) => void;
  loadProfiles: () => Promise<void>;
  setActiveProfile: (profile: Profile | null) => void;
}

function formatRelative(iso?: string): string | null {
  if (!iso) return null;
  const then = new Date(iso).getTime();
  if (isNaN(then)) return null;
  const ms = Date.now() - then;
  const mins = Math.round(ms / 60000);
  if (mins < 1) return 'just now';
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  if (days < 7) return `${days}d ago`;
  if (days < 30) return `${Math.round(days / 7)}w ago`;
  return new Date(iso).toLocaleDateString();
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
  connectedServiceOrg,
  destFqdn, setDestFqdn,
  destJwt, setDestJwt,
  destApiUsername, setDestApiUsername,
  destServiceOrgId, setDestServiceOrgId,
  destConnectionStatus, setDestConnectionStatus,
  setDestServerUrl, setDestServerVersion,
  setDestConnectedServiceOrg,
  destConnectedServiceOrg,
  onConnect,
  onSelectProfile,
  onShowNewProfile,
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

  const renderProfileList = () => (
    <div className="card card-compact fade-in">
      <div className="card-header">
        <h2 className="card-title" style={{ fontSize: '0.95rem' }}>Profiles</h2>
      </div>
      <div className="profile-list" style={{ display: 'grid', gap: 4 }}>
        {profiles.length === 0 ? (
          <div className="empty">
            <div className="icon">∅</div>
            <div className="title">No saved profiles yet</div>
            <div className="desc">Profiles store your server FQDN, username, and SO so you can re-connect with one click.</div>
            <button className="btn btn-primary" style={{ fontSize: 12, padding: '7px 14px' }} onClick={onShowNewProfile}>
              + Create your first profile
            </button>
          </div>
        ) : (
          <>
            {profiles.map(profile => {
              const isActive = activeProfile?.name === profile.name;
              const lastUsed = formatRelative(profile.lastUsed);
              return (
                <div
                  key={profile.name}
                  className={`item ${isActive ? 'active' : ''}`}
                  onClick={() => onSelectProfile(profile)}
                >
                  <div className="col">
                    <div className="name" title={profile.name}>{profile.name}</div>
                    <div className="sub" title={profile.source.fqdn}>
                      {profile.type === 'migration' && profile.destination
                        ? `${profile.source.fqdn} → ${profile.destination.fqdn}`
                        : profile.source.fqdn}
                    </div>
                    {lastUsed && <div className="lastrun">Last used <strong style={{ color: 'var(--color-text-secondary)', fontWeight: 500 }}>{lastUsed}</strong></div>}
                  </div>
                  <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                    <button
                      title="Delete profile"
                      style={{ background: 'transparent', border: 'none', color: 'var(--color-text-muted)', cursor: 'pointer', padding: '4px 6px', fontSize: 14 }}
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
                    >×</button>
                  </div>
                </div>
              );
            })}
            <button className="new" onClick={onShowNewProfile}>+ New profile</button>
          </>
        )}
      </div>
    </div>
  );

  const renderConnectionCard = (label: string, isDest: boolean) => {
    const status = isDest ? destConnectionStatus : connectionStatus;
    const isConnected = status === 'connected';
    const fqdnVal = isDest ? destFqdn : fqdn;
    const setFqdnVal = isDest ? setDestFqdn : setFqdn;
    const userVal = isDest ? destApiUsername : apiUsername;
    const setUserVal = isDest ? setDestApiUsername : setApiUsername;
    const jwtVal = isDest ? destJwt : jwt;
    const setJwtVal = isDest ? setDestJwt : setJwt;
    const soVal = isDest ? destServiceOrgId : serviceOrgId;
    const setSoVal = isDest ? setDestServiceOrgId : setServiceOrgId;
    const handleConnect = isDest ? handleDestConnect : onConnect;

    return (
      <div className="card card-compact fade-in">
        <div className="card-header" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <h2 className="card-title">
            {label} <span className="header-accent">Connection</span>
          </h2>
          {isConnected && (
            <span className="conn-chip" style={{ fontSize: 11 }}>
              <span className="dot" />Connected
            </span>
          )}
        </div>
        <div className="form-group">
          <label className="form-label">Server FQDN</label>
          <input type="text" className="form-input" placeholder="ncentral.example.com" value={fqdnVal} onChange={e => setFqdnVal(e.target.value)} />
        </div>
        <div className="form-group">
          <label className="form-label">API Username <span className="text-secondary" style={{ fontSize: '0.7em' }}>(required for user creation)</span></label>
          <input type="text" className="form-input" placeholder="admin@example.com" value={userVal} onChange={e => setUserVal(e.target.value)} />
        </div>
        <div className="form-group">
          <label className="form-label">JWT Token</label>
          <input type="password" className="form-input mono" placeholder="eyJhbGciOiJIUzI1NiIs..." value={jwtVal} onChange={e => setJwtVal(e.target.value)} />
        </div>

        <div className="form-group" style={{ borderTop: '1px dashed var(--color-border)', paddingTop: 'var(--space-md)' }}>
          <label className="form-label">
            Target Service Org
            {!isConnected && <span style={{ color: 'var(--color-text-muted)', fontWeight: 400, marginLeft: 6, fontSize: '0.7em' }}>(available after connect)</span>}
          </label>
          <ServiceOrgCombobox
            value={soVal}
            onChange={setSoVal}
            enabled={isConnected}
            placeholder="Select or type a service org…"
            initialName={isDest ? destConnectedServiceOrg?.name : connectedServiceOrg?.name}
          />
        </div>

        <button
          className={`btn ${isConnected ? 'btn-secondary' : 'btn-primary'} btn-lg`}
          style={{ width: '100%' }}
          onClick={handleConnect}
          disabled={status === 'connecting'}
        >
          {status === 'connecting'
            ? 'Connecting…'
            : isConnected
              ? `Reconnect ${isDest ? 'Destination' : (appMode === 'migrate' ? 'Source' : '')}`.trim()
              : `Connect ${isDest ? 'Destination' : (appMode === 'migrate' ? 'Source' : 'Now')}`}
        </button>
      </div>
    );
  };

  return (
    <div className="centered-dashboard fade-in">
      <div className={`setup-split${appMode === 'migrate' ? ' migrate' : ''}`}>
        {renderProfileList()}
        <div className={appMode === 'migrate' ? 'connection-pair' : ''} style={appMode === 'migrate' ? undefined : { display: 'grid', gap: 16 }}>
          {renderConnectionCard(appMode === 'migrate' ? 'Source' : 'Direct', false)}
          {appMode === 'migrate' && renderConnectionCard('Destination', true)}
        </div>
      </div>
    </div>
  );
}
