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
import { ImportPanel } from './components/ImportPanel';
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
  const [, setServerVersion] = useState<string>('');
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
  const [appMode, setAppMode] = useState<'export' | 'migrate' | 'import'>('export');

  // Import state
  const [importCsvPath, setImportCsvPath] = useState<string>('');
  const [importResource, setImportResource] = useState<string>('customers');
  const [importDryRun, setImportDryRun] = useState<boolean>(true);
  const [lastImportResult, setLastImportResult] = useState<import('./types').ImportResult | null>(null);

  // Destination state (for migration)
  const [destConnectionStatus, setDestConnectionStatus] = useState<ConnectionStatus>('disconnected');
  const [, setDestServerVersion] = useState<string>('');
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
    listen<ProgressUpdate>('import-progress', (event) => {
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
    setServerUrl('');
    setConnectedServiceOrg(null);
    setDestConnectionStatus('disconnected');
    setDestServerVersion('');
    setDestServerUrl('');
    setDestConnectedServiceOrg(null);
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
        users: selectedTypes.has('users'),
        deviceAssets: selectedTypes.has('device_assets')
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

  const handleImport = async (overrideDryRun?: boolean) => {
    if (!serviceOrgId) { addLog('error', 'Please enter Service Organization ID'); return; }
    if (!importCsvPath) { addLog('error', 'Please select a CSV file'); return; }
    if (!importResource) { addLog('error', 'Please select a resource to import'); return; }

    const dryRun = overrideDryRun !== undefined ? overrideDryRun : importDryRun;

    setCurrentStep('exporting');
    setProgress(null);
    setLastImportResult(null);
    addLog('info', `Starting ${dryRun ? 'dry-run import' : 'live import'}: ${importResource} from ${importCsvPath}`);

    try {
      const result = await api.startImport(importResource, importCsvPath, parseInt(serviceOrgId), dryRun);
      setLastImportResult(result);
      addLog(result.success ? 'success' : 'warning', result.message);
    } catch (e) {
      addLog('error', `Import failed: ${e}`);
    } finally {
      setCurrentStep('complete');
    }
  };

  const handleApplyForReal = () => {
    void handleImport(false);
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
      {/* Slim header strip — only shown after the home page */}
      {currentStep !== 'home' && (
        <header className="header">
          <div className="header-title">
            <h1
              onClick={() => setCurrentStep('home')}
              style={{ cursor: 'pointer' }}
              title="Back to Home"
            >
              <span className="header-accent">N-xport</span>
            </h1>
            <span className="badge" style={{ background: 'var(--color-bg-tertiary)', color: 'var(--color-text-muted)', fontSize: '0.7rem' }}>v{appVersion}</span>
            <button
              className="btn btn-ghost"
              onClick={() => setCurrentStep('home')}
              style={{ padding: '4px 10px', fontSize: '0.8125rem', marginLeft: 8 }}
              title="Back to Home"
            >
              ← Home
            </button>
          </div>
          <div className="header-actions" style={{ gap: 12 }}>
            {appMode === 'migrate' ? (
              <>
                <span className={`conn-chip ${connectionStatus === 'connected' ? '' : 'disconnected'}`}>
                  <span className="dot" />
                  Source · {connectionStatus === 'connected' ? (serverUrl || 'Connected') : connectionStatus === 'connecting' ? 'Connecting…' : 'Disconnected'}
                </span>
                <span className={`conn-chip ${destConnectionStatus === 'connected' ? '' : 'disconnected'}`}>
                  <span className="dot" />
                  Dest · {destConnectionStatus === 'connected' ? (destServerUrl || 'Connected') : destConnectionStatus === 'connecting' ? 'Connecting…' : 'Disconnected'}
                </span>
              </>
            ) : (
              <span className={`conn-chip ${connectionStatus === 'connected' ? '' : 'disconnected'}`}>
                <span className="dot" />
                {connectionStatus === 'connected' ? 'Connected' : connectionStatus === 'connecting' ? 'Connecting…' : 'Disconnected'}
                {connectedServiceOrg && (
                  <>
                    <span className="sep" />
                    <span className="so">{connectedServiceOrg.name} · {connectedServiceOrg.id}</span>
                  </>
                )}
                {connectionStatus === 'connected' && (
                  <button className="disconnect" onClick={handleDisconnect} title="Disconnect">×</button>
                )}
              </span>
            )}

            {currentStep !== 'exporting' && currentStep !== 'complete' && (
              <button
                className="btn btn-primary"
                style={{ fontWeight: 600, minWidth: '140px' }}
                onClick={() => {
                  if (currentStep === 'setup') setCurrentStep('configure');
                  else if (currentStep === 'configure') {
                    if (appMode === 'migrate') handleMigrate();
                    else if (appMode === 'import') void handleImport();
                    else handleExport();
                  }
                }}
                disabled={
                  currentStep === 'setup'
                    ? (appMode === 'migrate' ? (connectionStatus !== 'connected' || destConnectionStatus !== 'connected') : connectionStatus !== 'connected')
                    : (appMode === 'migrate'
                        ? (!serviceOrgId || !destConnectedServiceOrg)
                        : appMode === 'import'
                          ? (!serviceOrgId || !importCsvPath || !importResource)
                          : (!serviceOrgId || !outputDir))
                }
              >
                {currentStep === 'setup'
                  ? <>Continue<span className="kbd">↵</span></>
                  : appMode === 'migrate'
                    ? <>Start migration<span className="kbd">↵</span></>
                    : appMode === 'import'
                      ? (importDryRun ? <>Run dry-run<span className="kbd">↵</span></> : <>Start import<span className="kbd">↵</span></>)
                      : <>Start export<span className="kbd">↵</span></>}
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
              onSelectImport={() => { setAppMode('import'); setCurrentStep('setup'); }}
            />
          )}

          {/* Step pips — clickable to jump back to a completed step */}
          {currentStep !== 'home' && (
            <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 12 }}>
              <div className="step-pips" title="Click any completed step to jump back">
                <button
                  className={`pip ${currentStep === 'setup' ? 'current' : 'done'}`}
                  onClick={() => setCurrentStep('setup')}
                >
                  <span className="dot" />Setup
                </button>
                <button
                  className={`pip ${
                    currentStep === 'configure' ? 'current'
                      : ['exporting', 'complete'].includes(currentStep) ? 'done'
                      : 'future'
                  }`}
                  onClick={() => {
                    if (['exporting', 'complete'].includes(currentStep)) setCurrentStep('configure');
                  }}
                >
                  <span className="dot" />Configure
                </button>
                <button
                  className={`pip ${['exporting', 'complete'].includes(currentStep) ? 'current' : 'future'}`}
                >
                  <span className="dot" />
                  {appMode === 'migrate' ? 'Migrate' : appMode === 'import' ? 'Import' : 'Export'}
                </button>
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
              serviceOrgId={serviceOrgId} setServiceOrgId={setServiceOrgId}
              connectionStatus={connectionStatus}
              connectedServiceOrg={connectedServiceOrg}
              destFqdn={destFqdn} setDestFqdn={setDestFqdn}
              destJwt={destJwt} setDestJwt={setDestJwt}
              destApiUsername={destApiUsername} setDestApiUsername={setDestApiUsername}
              destServiceOrgId={destServiceOrgId} setDestServiceOrgId={setDestServiceOrgId}
              destConnectionStatus={destConnectionStatus}
              setDestConnectionStatus={setDestConnectionStatus}
              setDestServerUrl={setDestServerUrl}
              setDestServerVersion={setDestServerVersion}
              setDestConnectedServiceOrg={setDestConnectedServiceOrg}
              destConnectedServiceOrg={destConnectedServiceOrg}
              onConnect={handleConnect}
              onSelectProfile={handleSelectProfile}
              onShowNewProfile={() => setShowNewProfile(true)}
              addLog={addLog}
              loadProfiles={loadProfiles}
              setActiveProfile={setActiveProfile}
            />
          )}

          {currentStep === 'configure' && appMode !== 'import' && (
            <ConfigurePanel
              appMode={appMode as 'export' | 'migrate'}
              serviceOrgId={serviceOrgId} setServiceOrgId={setServiceOrgId}
              outputDir={outputDir} setOutputDir={setOutputDir}
              exportTypes={exportTypes}
              selectedTypes={selectedTypes}
              exportFormats={exportFormats}
              onToggleExportType={toggleExportType}
              onToggleFormat={toggleFormat}
              onBrowseOutput={handleBrowseOutput}
              onBack={() => setCurrentStep('setup')}
              connectedServiceOrgName={connectedServiceOrg?.name}
            />
          )}

          {currentStep === 'configure' && appMode === 'import' && (
            <ImportPanel
              serviceOrgId={serviceOrgId}
              setServiceOrgId={setServiceOrgId}
              csvPath={importCsvPath}
              setCsvPath={setImportCsvPath}
              selectedResource={importResource}
              setSelectedResource={setImportResource}
              dryRun={importDryRun}
              setDryRun={setImportDryRun}
              onBack={() => setCurrentStep('setup')}
              addLog={addLog}
              connectedServiceOrgName={connectedServiceOrg?.name}
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
              lastImportResult={lastImportResult}
              targetSoLabel={connectedServiceOrg ? `${connectedServiceOrg.name} (${connectedServiceOrg.id})` : (serviceOrgId ? `SO ${serviceOrgId}` : undefined)}
              onApplyForReal={handleApplyForReal}
            />
          )}
        </main>
      </div>

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
