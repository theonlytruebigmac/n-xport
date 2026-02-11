import { useState, useEffect, useCallback, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getVersion } from '@tauri-apps/api/app';
import { open } from '@tauri-apps/plugin-dialog';
import './index.css';
import * as api from './api';
import { UpdateBanner } from './useUpdateChecker';
import { HomePanel } from './components/HomePanel';
import { SetupPanel } from './components/SetupPanel';
import { ConfigurePanel } from './components/ConfigurePanel';
import { ProgressPanel } from './components/ProgressPanel';
import { NewProfileModal } from './components/NewProfileModal';
import type {
  Profile,
  ConnectionStatus,
  ExportOptions,
  MigrationOptions,
  ProgressUpdate,
  LogEntry,
  ExportType
} from './types';

function App() {
  // Connection state
  const [connectionStatus, setConnectionStatus] = useState<ConnectionStatus>('disconnected');
  const [serverVersion, setServerVersion] = useState<string>('');
  const [serverUrl, setServerUrl] = useState<string>('');
  const [connectedServiceOrg, setConnectedServiceOrg] = useState<{ id: number, name: string } | null>(null);

  // Profile state
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [activeProfile, setActiveProfile] = useState<Profile | null>(null);
  const [showNewProfile, setShowNewProfile] = useState(false);

  // Form state
  const [fqdn, setFqdn] = useState('');
  const [jwt, setJwt] = useState('');
  const [apiUsername, setApiUsername] = useState('');
  const [apiPassword, setApiPassword] = useState('');
  const [serviceOrgId, setServiceOrgId] = useState('');
  const [outputDir, setOutputDir] = useState('../nc_export');
  const [newProfileName, setNewProfileName] = useState('');

  // Export state
  const [exportTypes, setExportTypes] = useState<ExportType[]>([]);
  const [selectedTypes, setSelectedTypes] = useState<Set<string>>(new Set());
  const [exportFormats, setExportFormats] = useState<Set<string>>(new Set(['csv']));
  const [progress, setProgress] = useState<ProgressUpdate | null>(null);

  // Logs
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const isInitialLoad = useRef(false);

  // Workflow state
  const [currentStep, setCurrentStep] = useState<'home' | 'setup' | 'configure' | 'exporting' | 'complete'>('home');
  const [appMode, setAppMode] = useState<'export' | 'migrate'>('export');

  // Destination state (for migration)
  const [destConnectionStatus, setDestConnectionStatus] = useState<ConnectionStatus>('disconnected');
  const [destServerVersion, setDestServerVersion] = useState<string>('');
  const [destServerUrl, setDestServerUrl] = useState<string>('');
  const [destConnectedServiceOrg, setDestConnectedServiceOrg] = useState<{ id: number, name: string } | null>(null);
  const [destFqdn, setDestFqdn] = useState('');
  const [destJwt, setDestJwt] = useState('');
  const [destApiUsername, setDestApiUsername] = useState('');
  const [destServiceOrgId, setDestServiceOrgId] = useState('');

  // App version
  const [appVersion, setAppVersion] = useState<string>('...');

  // Load initial data
  useEffect(() => {
    if (!isInitialLoad.current) {
      isInitialLoad.current = true;
      loadProfiles();
      loadExportTypes();
      setupEventListeners();
      getVersion().then(v => setAppVersion(v)).catch(() => setAppVersion('dev'));
    }
  }, []);

  const loadProfiles = async () => {
    try {
      const profs = await api.getProfiles();
      setProfiles(profs);
      const active = await api.getActiveProfile();
      if (active) {
        setActiveProfile(active);
        setFqdn(active.source.fqdn);
        if (active.source.serviceOrgId) {
          setServiceOrgId(active.source.serviceOrgId.toString());
        }

        try {
          const storedJwt = await api.getCredentials(active.name);
          if (storedJwt) setJwt(storedJwt);
          const storedPwd = await api.getPassword(active.name);
          if (storedPwd) setApiPassword(storedPwd);
          setApiUsername(active.source.username || '');
        } catch (e) {
          // Ignore error loading creds
        }

        if (active.type === 'migration' && active.destination) {
          setAppMode('migrate');
          setDestFqdn(active.destination.fqdn);
          if (active.destination.serviceOrgId) {
            setDestServiceOrgId(active.destination.serviceOrgId.toString());
          }
          setDestApiUsername(active.destination.username || '');
          try {
            const storedDestJwt = await api.getCredentials(`${active.name}_dest`);
            if (storedDestJwt) setDestJwt(storedDestJwt);
          } catch (e) {
            // ignore
          }
        }

        addLog('info', `Profile "${active.name}" loaded. Click Export or Migrate to continue.`);
      }
    } catch (e) {
      addLog('error', `Failed to load profiles: ${e}`);
    }
  };

  const loadExportTypes = async () => {
    try {
      const types = await api.getExportTypes();
      setExportTypes(types);
      const defaults = new Set(types.filter(t => t.default).map(t => t.id));
      setSelectedTypes(defaults);
    } catch (e) {
      addLog('error', `Failed to load export types: ${e}`);
    }
  };

  const setupEventListeners = () => {
    listen<ProgressUpdate>('export-progress', (event) => {
      setProgress(event.payload);
      addLog('info', `${event.payload.phase}: ${event.payload.message}`);
    });
    listen<{ level: string; message: string }>('backend-log', (event) => {
      const level = event.payload.level as LogEntry['level'];
      addLog(level, event.payload.message);
    });
  };

  const addLog = useCallback((level: LogEntry['level'], message: string) => {
    setLogs(prev => {
      if (prev.length > 0 && prev[prev.length - 1].message === message) return prev;
      return [...prev.slice(-1999), { timestamp: new Date(), level, message }];
    });
  }, []);

  const handleConnect = async () => {
    if (!fqdn || !jwt) {
      addLog('error', 'Please enter server FQDN and JWT token');
      if (!apiUsername || !apiPassword) {
        addLog('error', 'API Username and Password are required');
        return;
      }
      return;
    }

    setConnectionStatus('connecting');
    addLog('info', `Connecting to ${fqdn}...`);

    try {
      const result = await api.testConnection(fqdn, jwt, apiUsername);
      if (result.success) {
        setConnectionStatus('connected');
        setServerVersion(result.serverVersion || '');
        setServerUrl(result.serverUrl || fqdn);

        let finalSoId = result.serviceOrgId;
        let finalSoName = result.serviceOrgName;

        if (serviceOrgId) {
          const id = parseInt(serviceOrgId);
          if (!isNaN(id)) {
            finalSoId = id;
            if (finalSoId !== result.serviceOrgId) {
              try {
                const info = await api.getServiceOrgInfo(finalSoId);
                finalSoName = info.name;
              } catch {
                finalSoName = `Unknown (ID: ${finalSoId})`;
              }
            }
          }
        } else if (result.serviceOrgId) {
          setServiceOrgId(result.serviceOrgId.toString());
        }

        if (finalSoId && finalSoName) {
          setConnectedServiceOrg({ id: finalSoId, name: finalSoName });
          addLog('info', `Target Service Org: ${finalSoName} (ID: ${finalSoId})`);
        }

        addLog('success', `Connected to ${result.serverUrl || fqdn}`);
        if (result.serverVersion) addLog('info', `Server version: ${result.serverVersion}`);

        if (activeProfile) {
          await api.saveCredentials(activeProfile.name, jwt);
          addLog('info', 'Credentials saved to keychain');
        }
      } else {
        setConnectionStatus('error');
        addLog('error', result.message);
      }
    } catch (e) {
      setConnectionStatus('error');
      addLog('error', `Connection failed: ${e}`);
    }
  };

  const handleDisconnect = async () => {
    await api.disconnect();
    setConnectionStatus('disconnected');
    setServerVersion('');
    addLog('info', 'Disconnected');
    setCurrentStep('setup');
  };

  const handleSaveProfile = async () => {
    if (!newProfileName || !fqdn) {
      addLog('error', 'Please enter profile name and server FQDN');
      return;
    }

    try {
      const isMigration = appMode === 'migrate' && destFqdn;
      const profile: Profile = {
        name: newProfileName,
        type: isMigration ? 'migration' : 'export',
        source: {
          fqdn: fqdn,
          username: apiUsername,
          serviceOrgId: serviceOrgId ? parseInt(serviceOrgId) : undefined
        },
        destination: isMigration ? {
          fqdn: destFqdn,
          username: destApiUsername,
          serviceOrgId: destConnectedServiceOrg?.id || (destServiceOrgId ? parseInt(destServiceOrgId) : undefined)
        } : undefined,
        lastUsed: new Date().toISOString()
      };

      await api.saveProfile(profile);
      await api.setActiveProfile(newProfileName);

      if (jwt) {
        await api.saveCredentials(newProfileName, jwt);
        addLog('debug', `Saved source credentials for ${newProfileName}`);
      }
      if (isMigration && destJwt) {
        await api.saveCredentials(`${newProfileName}_dest`, destJwt);
        addLog('debug', `Saved destination credentials for ${newProfileName}_dest`);
      }

      await loadProfiles();
      setActiveProfile(profile);
      addLog('success', `Profile "${newProfileName}" saved`);
      setNewProfileName('');
      setShowNewProfile(false);
    } catch (e) {
      addLog('error', `Failed to save profile: ${e}`);
    }
  };

  const handleSelectProfile = async (profile: Profile) => {
    setActiveProfile(profile);
    setFqdn(profile.source.fqdn);
    if (profile.source.serviceOrgId) {
      setServiceOrgId(profile.source.serviceOrgId.toString());
    } else {
      setServiceOrgId('');
    }

    if (profile.type === 'migration') {
      setAppMode('migrate');
      if (profile.destination) {
        setDestFqdn(profile.destination.fqdn);
        if (profile.destination.serviceOrgId) {
          setDestServiceOrgId(profile.destination.serviceOrgId.toString());
        }
      }
    } else {
      setAppMode('export');
    }

    try {
      const storedJwt = await api.getCredentials(profile.name);
      if (storedJwt) {
        setJwt(storedJwt);
        const storedPwd = await api.getPassword(profile.name);
        setApiPassword(storedPwd || '');
        setApiUsername(profile.source.username || '');
      } else {
        setJwt('');
        setApiUsername('');
        setApiPassword('');
      }
    } catch {
      setJwt('');
      setApiUsername('');
      setApiPassword('');
    }

    if (profile.type === 'migration') {
      try {
        const storedDestJwt = await api.getCredentials(`${profile.name}_dest`);
        if (storedDestJwt) {
          setDestJwt(storedDestJwt);
          setDestApiUsername(profile.destination?.username || '');

          // Auto-reconnect destination server
          if (profile.destination?.fqdn) {
            setDestConnectionStatus('connecting');
            addLog('info', `Connecting to destination ${profile.destination.fqdn}...`);
            try {
              const destResult = await api.connectDestination(
                profile.destination.fqdn,
                storedDestJwt,
                profile.destination.username
              );
              if (destResult.success) {
                setDestConnectionStatus('connected');
                setDestServerUrl(destResult.serverUrl || profile.destination.fqdn);
                setDestServerVersion(destResult.serverVersion || '');
                if (destResult.serviceOrgId && destResult.serviceOrgName) {
                  setDestConnectedServiceOrg({ id: destResult.serviceOrgId, name: destResult.serviceOrgName });
                }
                addLog('success', `Destination connected: ${destResult.serverUrl || profile.destination.fqdn}`);
              } else {
                setDestConnectionStatus('disconnected');
                addLog('warning', 'Destination credentials expired or invalid. Please reconnect.');
              }
            } catch (destErr) {
              setDestConnectionStatus('disconnected');
              addLog('warning', `Destination auto-connect failed: ${destErr}`);
            }
          }
        } else {
          setDestJwt('');
          setDestApiUsername('');
        }
      } catch {
        setDestJwt('');
        setDestApiUsername('');
      }
    }

    try {
      await api.setActiveProfile(profile.name);
      const hasCreds = await api.hasCredentials(profile.name);
      if (hasCreds) {
        setConnectionStatus('connecting');
        addLog('info', `Connecting with saved credentials for ${profile.name}...`);
        const result = await api.connectWithProfile(profile.name, profile.source.fqdn);
        if (result.success) {
          setConnectionStatus('connected');
          setServerVersion(result.serverVersion || '');
          setServerUrl(result.serverUrl || profile.source.fqdn);

          let finalSoId = result.serviceOrgId;
          let finalSoName = result.serviceOrgName;

          if (profile.source.serviceOrgId) {
            finalSoId = profile.source.serviceOrgId;
            if (finalSoId !== result.serviceOrgId && finalSoId) {
              try {
                const info = await api.getServiceOrgInfo(finalSoId);
                finalSoName = info.name;
              } catch {
                finalSoName = `Unknown (ID: ${finalSoId})`;
              }
            }
          } else if (result.serviceOrgId) {
            setServiceOrgId(result.serviceOrgId.toString());
          }

          if (finalSoId && finalSoName) {
            setConnectedServiceOrg({ id: finalSoId, name: finalSoName });
            addLog('info', `Target Service Org: ${finalSoName} (ID: ${finalSoId})`);
          }

          addLog('success', `Connected to ${result.serverUrl || profile.source.fqdn}`);
          if (result.serverVersion) addLog('info', `Server version: ${result.serverVersion}`);
        } else {
          setConnectionStatus('disconnected');
          addLog('warning', 'Saved credentials expired or invalid. Please enter JWT token.');
        }
      } else {
        addLog('info', `Selected profile "${profile.name}". Please enter JWT token to connect.`);
      }
    } catch (e) {
      setConnectionStatus('disconnected');
      addLog('error', `Failed to connect: ${e}`);
    }
  };

  const handleBrowseOutput = async () => {
    const selected = await open({ directory: true, title: 'Select Export Directory' });
    if (selected) setOutputDir(selected);
  };

  const handleOpenOutput = async () => {
    try {
      if (outputDir) {
        await api.openDirectory(outputDir);
        addLog('info', `Opened directory: ${outputDir}`);
      }
    } catch (e) {
      addLog('error', `Failed to open directory (check if it exists): ${e}`);
    }
  };

  const handleExport = async () => {
    if (!serviceOrgId) { addLog('error', 'Please enter Service Organization ID'); return; }
    if (selectedTypes.size === 0) { addLog('error', 'Please select at least one data type to export'); return; }

    setCurrentStep('exporting');
    setProgress(null);
    addLog('info', 'Starting export...');

    try {
      const options: ExportOptions = {
        serviceOrgs: selectedTypes.has('service_orgs'),
        customers: selectedTypes.has('customers'),
        sites: selectedTypes.has('sites'),
        devices: selectedTypes.has('devices'),
        accessGroups: selectedTypes.has('access_groups'),
        userRoles: selectedTypes.has('user_roles'),
        orgProperties: selectedTypes.has('org_properties'),
        deviceProperties: selectedTypes.has('device_properties'),
        users: selectedTypes.has('users')
      };

      const result = await api.startExport(outputDir, options, Array.from(exportFormats), parseInt(serviceOrgId));
      if (result.success) {
        addLog('success', result.message);
        addLog('info', `Files: ${result.filesCreated.join(', ')}`);
      } else {
        addLog('error', result.message);
      }
      // Surface any warnings
      if (result.warnings && result.warnings.length > 0) {
        for (const w of result.warnings) {
          addLog('warning', w);
        }
      }
      // Surface any errors
      if (result.errors && result.errors.length > 0) {
        for (const e of result.errors) {
          addLog('error', e);
        }
      }
    } catch (e) {
      addLog('error', `Export failed: ${e}`);
    } finally {
      setCurrentStep('complete');
    }
  };

  const handleMigrate = async () => {
    if (!serviceOrgId || !destConnectedServiceOrg) {
      addLog('error', 'Please ensure both Source and Destination Service Org IDs are available');
      return;
    }

    setCurrentStep('exporting');
    setProgress(null);
    addLog('info', 'Starting migration...');

    try {
      const options: MigrationOptions = {
        customers: selectedTypes.has('customers'),
        userRoles: selectedTypes.has('user_roles'),
        accessGroups: selectedTypes.has('access_groups'),
        users: selectedTypes.has('users'),
        orgProperties: selectedTypes.has('org_properties'),
      };

      const actualDestSoId = destServiceOrgId ? parseInt(destServiceOrgId) : destConnectedServiceOrg.id;
      addLog('info', `Starting migration: Source SO ${parseInt(serviceOrgId)} → Destination SO ${actualDestSoId}`);

      const result = await api.startMigration(options, parseInt(serviceOrgId), actualDestSoId);
      if (result.success) {
        addLog('success', result.message);
      } else {
        addLog('error', result.message);
      }
    } catch (e) {
      addLog('error', `Migration failed: ${e}`);
    } finally {
      setCurrentStep('complete');
    }
  };

  const toggleExportType = (id: string) => {
    setSelectedTypes(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleFormat = (format: string) => {
    setExportFormats(prev => {
      const next = new Set(prev);
      if (next.has(format)) { if (next.size > 1) next.delete(format); }
      else next.add(format);
      return next;
    });
  };

  return (
    <div className="app">
      {/* Header - only show when not on home page */}
      {currentStep !== 'home' && (
        <header className="header">
          <div className="header-title">
            <h1><span className="header-accent">N-xport</span> Data Tool</h1>
            <span className="badge" style={{ background: 'var(--color-bg-tertiary)', color: 'var(--color-text-muted)', fontSize: '0.7rem' }}>v{appVersion}</span>
          </div>
          <div className="header-actions">
            <div className="header-status">
              {appMode === 'migrate' ? (
                <div style={{ display: 'flex', gap: 'var(--space-md)', alignItems: 'center' }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--space-xs)' }}>
                    <span className="badge badge-info">Source</span>
                    <div className={`status-indicator ${connectionStatus === 'connected' ? 'connected' : ''}`} />
                    <span className="form-label" style={{ marginBottom: 0, fontSize: '0.75rem' }}>
                      {serverUrl || (connectionStatus === 'connecting' ? 'Connecting...' : 'Disconnected')}
                      {serverVersion && ` (v${serverVersion})`}
                    </span>
                  </div>
                  <div style={{ height: '16px', width: '1px', background: 'var(--color-border)' }} />
                  <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--space-xs)' }}>
                    <span className="badge badge-info">Dest</span>
                    <div className={`status-indicator ${destConnectionStatus === 'connected' ? 'connected' : ''}`} />
                    <span className="form-label" style={{ marginBottom: 0, fontSize: '0.75rem' }}>
                      {destServerUrl || (destConnectionStatus === 'connecting' ? 'Connecting...' : 'Disconnected')}
                      {destServerVersion && ` (v${destServerVersion})`}
                    </span>
                  </div>
                </div>
              ) : (
                <>
                  <div className={`status-indicator ${connectionStatus === 'connected' ? 'connected' : ''}`} />
                  <span className="form-label" style={{ marginBottom: 0, fontSize: '0.8125rem' }}>
                    {connectionStatus === 'connected'
                      ? `${serverUrl || 'Connected'} ${connectedServiceOrg ? `· ${connectedServiceOrg.name}` : ''}`
                      : connectionStatus === 'connecting' ? 'Connecting...' : 'Disconnected'}
                  </span>
                  {serverVersion && (
                    <span className="badge badge-info" style={{ marginLeft: 'var(--space-sm)' }}>v{serverVersion}</span>
                  )}
                </>
              )}
            </div>

            {currentStep !== 'exporting' && currentStep !== 'complete' && (
              <button
                className="btn btn-primary"
                style={{ fontWeight: 700, minWidth: '120px' }}
                onClick={() => {
                  if (currentStep === 'setup') setCurrentStep('configure');
                  else if (currentStep === 'configure') {
                    if (appMode === 'migrate') handleMigrate();
                    else handleExport();
                  }
                }}
                disabled={
                  currentStep === 'setup'
                    ? (appMode === 'migrate' ? (connectionStatus !== 'connected' || destConnectionStatus !== 'connected') : connectionStatus !== 'connected')
                    : (appMode === 'migrate' ? (!serviceOrgId || !destConnectedServiceOrg) : (!serviceOrgId || !outputDir))
                }
              >
                {currentStep === 'setup' ? 'Next: Configure' : (appMode === 'migrate' ? 'Start Migration' : 'Start Export')}
              </button>
            )}
          </div>
        </header>
      )}

      <div className="main-content workflow-container">
        <main className="content-area centered-dashboard">
          <UpdateBanner />

          {currentStep === 'home' && (
            <HomePanel
              appVersion={appVersion}
              onSelectExport={() => { setAppMode('export'); setCurrentStep('setup'); }}
              onSelectMigrate={() => { setAppMode('migrate'); setCurrentStep('setup'); }}
            />
          )}

          {/* Workflow Indicator */}
          {currentStep !== 'home' && (
            <div className="step-indicator">
              <div className={`step-item ${currentStep === 'setup' ? 'active' : 'completed'}`}>
                <div className="step-number">1</div>
                <div className="step-label">Setup</div>
              </div>
              <div className={`step-line ${['configure', 'exporting', 'complete'].includes(currentStep) ? 'active' : ''}`} />
              <div className={`step-item ${currentStep === 'configure' ? 'active' : ['exporting', 'complete'].includes(currentStep) ? 'completed' : ''}`}>
                <div className="step-number">2</div>
                <div className="step-label">Configure</div>
              </div>
              <div className={`step-line ${['exporting', 'complete'].includes(currentStep) ? 'active' : ''}`} />
              <div className={`step-item ${['exporting', 'complete'].includes(currentStep) ? 'active' : ''}`}>
                <div className="step-number">3</div>
                <div className="step-label">Export</div>
              </div>
            </div>
          )}

          {currentStep === 'setup' && (
            <SetupPanel
              appMode={appMode}
              profiles={profiles}
              activeProfile={activeProfile}
              fqdn={fqdn} setFqdn={setFqdn}
              jwt={jwt} setJwt={setJwt}
              apiUsername={apiUsername} setApiUsername={setApiUsername}
              apiPassword={apiPassword} setApiPassword={setApiPassword}
              serviceOrgId={serviceOrgId} setServiceOrgId={setServiceOrgId}
              connectionStatus={connectionStatus}
              destFqdn={destFqdn} setDestFqdn={setDestFqdn}
              destJwt={destJwt} setDestJwt={setDestJwt}
              destApiUsername={destApiUsername} setDestApiUsername={setDestApiUsername}
              destServiceOrgId={destServiceOrgId} setDestServiceOrgId={setDestServiceOrgId}
              destConnectionStatus={destConnectionStatus}
              setDestConnectionStatus={setDestConnectionStatus}
              setDestServerUrl={setDestServerUrl}
              setDestServerVersion={setDestServerVersion}
              setDestConnectedServiceOrg={setDestConnectedServiceOrg}
              onConnect={handleConnect}
              onSelectProfile={handleSelectProfile}
              onShowNewProfile={() => setShowNewProfile(true)}
              onBack={() => setCurrentStep('home')}
              addLog={addLog}
              loadProfiles={loadProfiles}
              setActiveProfile={setActiveProfile}
            />
          )}

          {currentStep === 'configure' && (
            <ConfigurePanel
              appMode={appMode}
              serviceOrgId={serviceOrgId} setServiceOrgId={setServiceOrgId}
              outputDir={outputDir} setOutputDir={setOutputDir}
              exportTypes={exportTypes}
              selectedTypes={selectedTypes}
              exportFormats={exportFormats}
              onToggleExportType={toggleExportType}
              onToggleFormat={toggleFormat}
              onBrowseOutput={handleBrowseOutput}
              onBack={() => setCurrentStep('setup')}
            />
          )}

          {(currentStep === 'exporting' || currentStep === 'complete') && (
            <ProgressPanel
              currentStep={currentStep}
              appMode={appMode}
              progress={progress}
              logs={logs}
              addLog={addLog}
              onOpenOutput={handleOpenOutput}
              onNewExport={() => setCurrentStep('configure')}
              onCancel={async () => {
                addLog('warning', 'Cancellation requested...');
                await api.cancelExport();
              }}
            />
          )}
        </main>
      </div>

      {/* Connection Drawer */}
      {connectionStatus === 'connected' && currentStep !== 'setup' && (
        <div className="connection-fixed-status">
          <div className="status-indicator connected" />
          <span onClick={() => setCurrentStep('setup')}>Connected to {serverUrl}</span>
          <button
            className="btn btn-ghost"
            style={{ padding: '0 4px', marginLeft: 'var(--space-sm)', color: 'var(--color-error)' }}
            onClick={handleDisconnect}
          >
            Disconnect
          </button>
        </div>
      )}

      {/* New Profile Modal */}
      {showNewProfile && (
        <NewProfileModal
          appMode={appMode}
          newProfileName={newProfileName}
          setNewProfileName={setNewProfileName}
          fqdn={fqdn}
          setFqdn={setFqdn}
          destFqdn={destFqdn}
          connectedServiceOrg={connectedServiceOrg}
          destConnectedServiceOrg={destConnectedServiceOrg}
          onSave={handleSaveProfile}
          onClose={() => setShowNewProfile(false)}
        />
      )}
    </div>
  );
}

export default App;
