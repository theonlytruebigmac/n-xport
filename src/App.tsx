import { useState, useEffect, useCallback, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getVersion } from '@tauri-apps/api/app';
import { open } from '@tauri-apps/plugin-dialog';
import './index.css';
import * as api from './api';
import { UpdateBanner } from './useUpdateChecker';
import type {
  Profile,
  ConnectionStatus,
  ExportOptions,
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
  const logRef = useRef<HTMLDivElement>(null);

  // Auto-scroll logs
  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [logs]);

  // Workflow state
  const [currentStep, setCurrentStep] = useState<'setup' | 'configure' | 'exporting' | 'complete'>('setup');
  const [appMode, setAppMode] = useState<'export' | 'migrate'>('export');

  // Destination state (for migration)
  const [destConnectionStatus, setDestConnectionStatus] = useState<ConnectionStatus>('disconnected');
  const [destServerVersion, setDestServerVersion] = useState<string>('');
  const [destServerUrl, setDestServerUrl] = useState<string>('');
  const [destConnectedServiceOrg, setDestConnectedServiceOrg] = useState<{ id: number, name: string } | null>(null);
  const [destFqdn, setDestFqdn] = useState('');
  const [destJwt, setDestJwt] = useState('');
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
        setFqdn(active.fqdn);
        if (active.serviceOrgId) {
          setServiceOrgId(active.serviceOrgId.toString());
        }

        // Also load JWT for the active profile
        try {
          const storedJwt = await api.getCredentials(active.name);
          if (storedJwt) setJwt(storedJwt);
        } catch (e) {
          // Ignore error loading creds
        }

        // Try to connect automagically
        const has = await api.hasCredentials(active.name);
        if (has) {
          setConnectionStatus('connecting');
          addLog('info', `Connecting with saved credentials for ${active.name}...`);
          const result = await api.connectWithProfile(active.name, active.fqdn);
          if (result.success) {
            setConnectionStatus('connected');
            setServerVersion(result.serverVersion || '');
            setServerUrl(result.serverUrl || active.fqdn);
            // Resolve Service Org
            let finalSoId = result.serviceOrgId;
            let finalSoName = result.serviceOrgName;

            // Check if profile has a specific SO ID
            if (active.serviceOrgId) {
              finalSoId = active.serviceOrgId;
              if (finalSoId !== result.serviceOrgId) {
                try {
                  const info = await api.getServiceOrgInfo(finalSoId);
                  finalSoName = info.name;
                } catch (e) {
                  finalSoName = `Unknown (ID: ${finalSoId})`;
                }
              }
            } else if (result.serviceOrgId) {
              // Profile has no ID, but API returned one - auto-fill form
              setServiceOrgId(result.serviceOrgId.toString());
            }

            if (finalSoId && finalSoName) {
              setConnectedServiceOrg({ id: finalSoId, name: finalSoName });
              addLog('info', `Target Service Org: ${finalSoName} (ID: ${finalSoId})`);
            }

            addLog('success', `Connected to ${result.serverUrl || active.fqdn}`);
            if (result.serverVersion) addLog('info', `Server version: ${result.serverVersion}`);
            setCurrentStep('configure');
          } else {
            setConnectionStatus('disconnected');
            addLog('warning', 'Saved credentials expired or invalid. Please enter JWT token.');
          }
        } else {
          addLog('info', `Selected profile "${active.name}". Please enter JWT token to connect.`);
        }
      }
    } catch (e) {
      addLog('error', `Failed to load profiles: ${e}`);
    }
  };

  const loadExportTypes = async () => {
    try {
      const types = await api.getExportTypes();
      setExportTypes(types);
      // Select defaults
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
  };

  const addLog = useCallback((level: LogEntry['level'], message: string) => {
    setLogs(prev => {
      // Don't add duplicate consecutive logs
      if (prev.length > 0 && prev[prev.length - 1].message === message) {
        return prev;
      }
      return [...prev.slice(-99), {
        timestamp: new Date(),
        level,
        message
      }];
    });
  }, []);

  const handleConnect = async () => {
    if (!fqdn || !jwt) {
      addLog('error', 'Please enter server FQDN and JWT token');
      return;
    }

    setConnectionStatus('connecting');
    addLog('info', `Connecting to ${fqdn}...`);

    try {
      const result = await api.testConnection(fqdn, jwt);

      if (result.success) {
        setConnectionStatus('connected');
        setServerVersion(result.serverVersion || '');
        setServerUrl(result.serverUrl || fqdn);
        // Resolve Service Org
        let finalSoId = result.serviceOrgId;
        let finalSoName = result.serviceOrgName;

        if (serviceOrgId) {
          // If user entered an ID, use it and look up the name
          const id = parseInt(serviceOrgId);
          if (!isNaN(id)) {
            finalSoId = id;
            if (finalSoId !== result.serviceOrgId) {
              try {
                const info = await api.getServiceOrgInfo(finalSoId);
                finalSoName = info.name;
              } catch (e) {
                // If lookup fails, just show ID
                finalSoName = `Unknown (ID: ${finalSoId})`;
              }
            }
          }
        } else if (result.serviceOrgId) {
          // Auto-fill if empty
          setServiceOrgId(result.serviceOrgId.toString());
        }

        if (finalSoId && finalSoName) {
          setConnectedServiceOrg({ id: finalSoId, name: finalSoName });
          addLog('info', `Target Service Org: ${finalSoName} (ID: ${finalSoId})`);
        }

        addLog('success', `Connected to ${result.serverUrl || fqdn}`);
        if (result.serverVersion) addLog('info', `Server version: ${result.serverVersion}`);

        // Save credentials if we have a profile
        if (activeProfile) {
          await api.saveCredentials(activeProfile.name, jwt);
          addLog('info', 'Credentials saved to keychain');
        }

        // Switch to configure step only in export mode
        if (appMode === 'export') {
          setCurrentStep('configure');
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
      const profile: Profile = {
        name: newProfileName,
        fqdn: fqdn,
        serviceOrgId: serviceOrgId ? parseInt(serviceOrgId) : undefined
      };

      await api.saveProfile(profile);
      await api.setActiveProfile(newProfileName);

      if (jwt) {
        await api.saveCredentials(newProfileName, jwt);
      }

      addLog('success', `Profile "${newProfileName}" saved`);
      setNewProfileName('');
      setShowNewProfile(false);
      await loadProfiles();
    } catch (e) {
      addLog('error', `Failed to save profile: ${e}`);
    }
  };

  const handleSelectProfile = async (profile: Profile) => {
    setActiveProfile(profile);
    setFqdn(profile.fqdn);
    if (profile.serviceOrgId) {
      setServiceOrgId(profile.serviceOrgId.toString());
    }

    // Try to load JWT from keychain to populate the field
    try {
      const storedJwt = await api.getCredentials(profile.name);
      if (storedJwt) {
        setJwt(storedJwt);
      } else {
        setJwt('');
      }
    } catch (e) {
      setJwt('');
    }

    try {
      await api.setActiveProfile(profile.name);

      // Try to connect with saved credentials
      const hasCreds = await api.hasCredentials(profile.name);
      if (hasCreds) {
        setConnectionStatus('connecting');
        addLog('info', `Connecting with saved credentials for ${profile.name}...`);
        const result = await api.connectWithProfile(profile.name, profile.fqdn);
        if (result.success) {
          setConnectionStatus('connected');
          setServerVersion(result.serverVersion || '');
          setServerUrl(result.serverUrl || profile.fqdn);
          // Resolve Service Org
          let finalSoId = result.serviceOrgId;
          let finalSoName = result.serviceOrgName;

          // Check if profile has a specific SO ID
          if (profile.serviceOrgId) {
            finalSoId = profile.serviceOrgId;
            if (finalSoId !== result.serviceOrgId) {
              try {
                const info = await api.getServiceOrgInfo(finalSoId);
                finalSoName = info.name;
              } catch (e) {
                finalSoName = `Unknown (ID: ${finalSoId})`;
              }
            }
          } else if (result.serviceOrgId) {
            // Profile has no ID, but API returned one - auto-fill form
            setServiceOrgId(result.serviceOrgId.toString());
          }

          if (finalSoId && finalSoName) {
            setConnectedServiceOrg({ id: finalSoId, name: finalSoName });
            addLog('info', `Target Service Org: ${finalSoName} (ID: ${finalSoId})`);
          }

          addLog('success', `Connected to ${result.serverUrl || profile.fqdn}`);
          if (result.serverVersion) addLog('info', `Server version: ${result.serverVersion}`);
          setCurrentStep('configure');
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
    const selected = await open({
      directory: true,
      title: 'Select Export Directory'
    });
    if (selected) {
      setOutputDir(selected);
    }
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
    if (!serviceOrgId) {
      addLog('error', 'Please enter Service Organization ID');
      return;
    }

    if (selectedTypes.size === 0) {
      addLog('error', 'Please select at least one data type to export');
      return;
    }

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

      const result = await api.startExport(
        outputDir,
        options,
        Array.from(exportFormats),
        parseInt(serviceOrgId)
      );

      if (result.success) {
        addLog('success', result.message);
        addLog('info', `Files: ${result.filesCreated.join(', ')}`);
      } else {
        addLog('error', result.message);
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
      const options: any = {
        customers: selectedTypes.has('customers'),
        userRoles: selectedTypes.has('user_roles'),
        accessGroups: selectedTypes.has('access_groups'),
        users: selectedTypes.has('users'),
        orgProperties: selectedTypes.has('org_properties'),
        deviceProperties: selectedTypes.has('device_properties'),
      };

      const result = await api.startMigration(
        options,
        parseInt(serviceOrgId),
        destConnectedServiceOrg.id
      );

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
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const toggleFormat = (format: string) => {
    setExportFormats(prev => {
      const next = new Set(prev);
      if (next.has(format)) {
        if (next.size > 1) next.delete(format);
      } else {
        next.add(format);
      }
      return next;
    });
  };

  return (
    <div className="app">
      {/* Header */}
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
                  <span className="form-label" style={{ marginBottom: 0, fontSize: '0.75rem' }}>{serverUrl || (connectionStatus === 'connecting' ? 'Connecting...' : 'Disconnected')}</span>
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
                  <span className="badge badge-info" style={{ marginLeft: 'var(--space-sm)' }}>
                    v{serverVersion}
                  </span>
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

      <div className="main-content workflow-container">
        <main className="content-area centered-dashboard">
          {/* Update Banner */}
          <UpdateBanner />

          {/* Workflow Indicator */}
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

          {currentStep === 'setup' && (
            <div className="centered-dashboard fade-in">
              {/* Mode Switcher */}
              <div className="card" style={{ padding: 'var(--space-sm)', marginBottom: 'var(--space-md)' }}>
                <div style={{ display: 'flex', gap: 'var(--space-xs)' }}>
                  <button
                    className={`btn ${appMode === 'export' ? 'btn-primary' : 'btn-ghost'}`}
                    style={{ flex: 1 }}
                    onClick={() => setAppMode('export')}
                  >
                    Export Mode
                  </button>
                  <button
                    className={`btn ${appMode === 'migrate' ? 'btn-primary' : 'btn-ghost'}`}
                    style={{ flex: 1 }}
                    onClick={() => setAppMode('migrate')}
                  >
                    Migration Mode
                  </button>
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
                        onClick={() => handleSelectProfile(profile)}
                      >
                        <div className="profile-info">
                          <span className="profile-name">{profile.name}</span>
                          <span className="profile-fqdn">{profile.fqdn}</span>
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
                    <button className="profile-card new" onClick={() => setShowNewProfile(true)}>
                      <span>+ New Profile</span>
                    </button>
                  </div>
                </div>
              </div>

              <div className={appMode === 'migrate' ? 'setup-grid' : ''}>
                {/* Source Connection / Direct Connection */}
                <div className="card card-compact fade-in">
                  <div className="card-header">
                    <h2 className="card-title">
                      {appMode === 'migrate' ? 'Source' : 'Direct'} <span className="header-accent">Connection</span>
                    </h2>
                  </div>
                  <div className="form-group">
                    <label className="form-label">Server FQDN</label>
                    <input
                      type="text"
                      className="form-input"
                      placeholder="ncentral.example.com"
                      value={fqdn}
                      onChange={e => setFqdn(e.target.value)}
                    />
                  </div>
                  <div className="form-group">
                    <label className="form-label">JWT Token</label>
                    <input
                      type="password"
                      className="form-input mono"
                      placeholder="eyJhbGciOiJIUzI1NiIs..."
                      value={jwt}
                      onChange={e => setJwt(e.target.value)}
                    />
                  </div>
                  <div className="form-group">
                    <label className="form-label">Target Service Org ID</label>
                    <input
                      type="number"
                      className="form-input"
                      placeholder="Service Org ID (optional)"
                      value={serviceOrgId}
                      onChange={e => setServiceOrgId(e.target.value)}
                    />
                  </div>
                  <button
                    className="btn btn-primary btn-lg"
                    style={{ width: '100%' }}
                    onClick={handleConnect}
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
                      <input
                        type="text"
                        className="form-input"
                        placeholder="destination.example.com"
                        value={destFqdn}
                        onChange={e => setDestFqdn(e.target.value)}
                      />
                    </div>
                    <div className="form-group">
                      <label className="form-label">JWT Token</label>
                      <input
                        type="password"
                        className="form-input mono"
                        placeholder="eyJhbGciOiJIUzI1NiIs..."
                        value={destJwt}
                        onChange={e => setDestJwt(e.target.value)}
                      />
                    </div>
                    <div className="form-group">
                      <label className="form-label">Target Service Org ID</label>
                      <input
                        type="number"
                        className="form-input"
                        placeholder="Service Org ID (optional)"
                        value={destServiceOrgId}
                        onChange={e => setDestServiceOrgId(e.target.value)}
                      />
                    </div>
                    <button
                      className="btn btn-primary btn-lg"
                      style={{ width: '100%' }}
                      onClick={async () => {
                        setDestConnectionStatus('connecting');
                        addLog('info', `Connecting to Destination: ${destFqdn}...`);
                        try {
                          const result = await api.testConnection(destFqdn, destJwt);
                          if (result.success) {
                            setDestConnectionStatus('connected');
                            setDestServerUrl(result.serverUrl || destFqdn);
                            setDestServerVersion(result.serverVersion || '');

                            // Resolve Destination Service Org
                            let finalDestSoId = result.serviceOrgId;
                            let finalDestSoName = result.serviceOrgName;

                            if (destServiceOrgId) {
                              // User specified a SO ID
                              const id = parseInt(destServiceOrgId);
                              if (!isNaN(id)) {
                                finalDestSoId = id;
                                if (finalDestSoId !== result.serviceOrgId) {
                                  try {
                                    const info = await api.getServiceOrgInfo(finalDestSoId);
                                    finalDestSoName = info.name;
                                  } catch (e) {
                                    finalDestSoName = `Unknown (ID: ${finalDestSoId})`;
                                  }
                                }
                              }
                            } else if (result.serviceOrgId) {
                              // Auto-fill if empty
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
                      }}
                      disabled={destConnectionStatus === 'connecting'}
                    >
                      {destConnectionStatus === 'connecting' ? 'Connecting...' : 'Connect Destination'}
                    </button>
                  </div>
                )}
              </div>
            </div>
          )}

          {currentStep === 'configure' && (
            <div className="card card-compact fade-in">
              <div className="card-header">
                <h2 className="card-title">Configure <span className="header-accent">{appMode === 'migrate' ? 'Migration' : 'Export'}</span></h2>
              </div>

              <div className="grid-2">
                <div className="form-group">
                  <label className="form-label">Target Service Org ID</label>
                  <input
                    type="number"
                    className="form-input"
                    value={serviceOrgId}
                    onChange={e => setServiceOrgId(e.target.value)}
                  />
                </div>
                {appMode !== 'migrate' && (
                  <div className="form-group">
                    <label className="form-label">Output Directory</label>
                    <div style={{ display: 'flex', gap: 'var(--space-sm)' }}>
                      <input
                        type="text"
                        className="form-input"
                        value={outputDir}
                        onChange={e => setOutputDir(e.target.value)}
                      />
                      <button className="btn btn-secondary" onClick={handleBrowseOutput}>Browse</button>
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
                      <input type="checkbox" checked={selectedTypes.has(type.id)} onChange={() => toggleExportType(type.id)} />
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
                        <input type="checkbox" checked={exportFormats.has(f)} onChange={() => toggleFormat(f)} />
                        <span style={{ textTransform: 'uppercase' }}>{f}</span>
                      </label>
                    ))}
                  </div>
                </div>
              )}

              <div style={{ display: 'flex', gap: 'var(--space-md)', marginTop: 'var(--space-md)' }}>
                <button className="btn btn-secondary btn-lg" style={{ flex: 1 }} onClick={() => setCurrentStep('setup')}>
                  Back to Setup
                </button>
              </div>
            </div>
          )}

          {(currentStep === 'exporting' || currentStep === 'complete') && (
            <div className="card fade-in">
              <div className="card-header">
                <h2 className="card-title">
                  {currentStep === 'exporting' ? (appMode === 'migrate' ? 'Migrating...' : 'Exporting...') : 'Complete'}
                </h2>
              </div>

              {progress && (
                <div className="progress-container large">
                  <div className="progress-bar">
                    <div className="progress-fill" style={{ width: `${progress.percent}%` }} />
                  </div>
                  <div className="progress-stats">
                    <span className="phase">{progress.phase}</span>
                    <span className="percent">{Math.round(progress.percent)}%</span>
                  </div>
                  <div className="progress-message">{progress.message}</div>
                </div>
              )}

              <div className="live-logs">
                <div className="logs-header">Activity Log</div>
                <div className="log-panel" ref={logRef}>
                  {logs.slice(-50).map((log, i) => (
                    <div key={i} className={`log-entry ${log.level}`}>
                      <span className="log-time">[{log.timestamp.toLocaleTimeString()}]</span> {log.message}
                    </div>
                  ))}
                </div>
              </div>

              {currentStep === 'complete' && (
                <div style={{ display: 'flex', gap: 'var(--space-md)', marginTop: 'var(--space-xl)' }}>
                  {appMode !== 'migrate' && (
                    <button className="btn btn-primary btn-lg" style={{ flex: 1 }} onClick={handleOpenOutput}>
                      View Export Folder
                    </button>
                  )}
                  <button className="btn btn-secondary btn-lg" style={{ flex: 1 }} onClick={() => setCurrentStep('configure')}>
                    {appMode === 'migrate' ? 'New Migration' : 'Start New Export'}
                  </button>
                </div>
              )}
            </div>
          )}
        </main>
      </div >

      {/* Connection Drawer */}
      {
        connectionStatus === 'connected' && currentStep !== 'setup' && (
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
        )
      }

      {/* New Profile Modal - at root level for proper z-index */}
      {showNewProfile && (
        <div className="modal-overlay" onClick={() => setShowNewProfile(false)}>
          <div className="card modal-content" onClick={e => e.stopPropagation()}>
            <div className="card-header">
              <h2 className="card-title">Create New Profile</h2>
            </div>
            <div className="form-group">
              <label className="form-label">Profile Name</label>
              <input
                type="text"
                className="form-input"
                placeholder="My Server"
                value={newProfileName}
                onChange={e => setNewProfileName(e.target.value)}
              />
            </div>
            <div className="form-group">
              <label className="form-label">Server FQDN</label>
              <input
                type="text"
                className="form-input"
                placeholder="ncentral.example.com"
                value={fqdn}
                onChange={e => setFqdn(e.target.value)}
              />
            </div>
            <div style={{ display: 'flex', gap: 'var(--space-md)' }}>
              <button className="btn btn-primary" onClick={handleSaveProfile}>Save Profile</button>
              <button className="btn btn-ghost" onClick={() => setShowNewProfile(false)}>Cancel</button>
            </div>
          </div>
        </div>
      )}
    </div >
  );
}

export default App;
