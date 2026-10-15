import { useState, useEffect, useCallback, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getVersion } from '@tauri-apps/api/app';
import { open } from '@tauri-apps/plugin-dialog';
import { downloadDir } from '@tauri-apps/api/path';
import './index.css';
import * as api from './api';
import { UpdateBanner } from './useUpdateChecker';
import { HomePanel } from './components/HomePanel';
import { SettingsPanel } from './components/SettingsPanel';
import { InstallerPanel } from './components/InstallerPanel';
import { PageHead } from './components/PageHead';
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
  ExportType,
  ExportResult,
  PasswordPolicy,
  InstallerTarget
} from './types';
import { DEFAULT_PASSWORD_POLICY, installerTargetKey, toMigrationOptions } from './types';

const OUTPUT_DIR_KEY = 'nxport.outputDir';

/** The host part of a server address, lower-cased, so "https://NC.example.com/" equals "nc.example.com". */
function serverHost(address: string): string {
  return address.trim().toLowerCase().replace(/^https?:\/\//, '').split(/[/?#]/)[0];
}

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
  const [serviceOrgId, setServiceOrgId] = useState('');
  const [outputDir, setOutputDirState] = useState('../nc_export');
  const [newProfileName, setNewProfileName] = useState('');

  // Export state
  const [exportTypes, setExportTypes] = useState<ExportType[]>([]);
  const [selectedTypes, setSelectedTypes] = useState<Set<string>>(new Set());
  const [exportFormats, setExportFormats] = useState<Set<string>>(new Set(['csv']));
  const [progress, setProgress] = useState<ProgressUpdate | null>(null);
  const [lastExportResult, setLastExportResult] = useState<ExportResult | null>(null);

  // Logs
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const isInitialLoad = useRef(false);

  // Workflow state
  const [currentStep, setCurrentStep] = useState<'home' | 'settings' | 'setup' | 'configure' | 'exporting' | 'complete'>('home');
  const [appMode, setAppMode] = useState<'export' | 'migrate' | 'import' | 'installers'>('export');

  // Import state
  // Installer scripts state
  const [installerTargets, setInstallerTargets] = useState<InstallerTarget[]>([]);
  const [installerSelected, setInstallerSelected] = useState<Set<string>>(new Set());

  const [importCsvPath, setImportCsvPath] = useState<string>('');
  const [importResource, setImportResource] = useState<string>('customers');
  const [importDryRun, setImportDryRun] = useState<boolean>(true);
  const [passwordPolicy, setPasswordPolicyState] = useState<PasswordPolicy>(DEFAULT_PASSWORD_POLICY);
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
      api.getSettings()
        .then(s => { if (s.passwordPolicy) setPasswordPolicyState(s.passwordPolicy); })
        .catch(() => { /* keep defaults */ });
    }
  }, []);

  // Where files are written: the folder used last time, or a folder inside Downloads. A path
  // relative to the app would land somewhere unpredictable (and often unwritable) once installed.
  const setOutputDir = useCallback((dir: string) => {
    setOutputDirState(dir);
    try { localStorage.setItem(OUTPUT_DIR_KEY, dir); } catch { /* remembering is a convenience */ }
  }, []);

  useEffect(() => {
    let current = true;
    (async () => {
      try {
        const saved = localStorage.getItem(OUTPUT_DIR_KEY);
        if (saved) { if (current) setOutputDirState(saved); return; }
      } catch { /* fall through to the default */ }
      try {
        const downloads = await downloadDir();
        const separator = downloads.includes('\\') ? '\\' : '/';
        if (current) setOutputDirState(`${downloads.replace(/[\\/]+$/, '')}${separator}nc_export`);
      } catch { /* keep the relative default */ }
    })();
    return () => { current = false; };
  }, []);

  // Saving after the last change (not on every keystroke) keeps rapid edits from racing
  // each other: each save reads and rewrites the whole settings file.
  const policySaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const setPasswordPolicy = useCallback((p: PasswordPolicy) => {
    setPasswordPolicyState(p);
    if (policySaveTimer.current) clearTimeout(policySaveTimer.current);
    policySaveTimer.current = setTimeout(() => {
      api.getSettings()
        .then(s => api.saveSettings({ ...s, passwordPolicy: p }))
        .catch(() => { /* non-fatal: in-memory value still applies for this session */ });
    }, 500);
  }, []);

  // Keep the source connection chip in sync with the SO dropdown while connected.
  // Debounced so manual numeric typing doesn't fire a lookup per keystroke.
  useEffect(() => {
    if (connectionStatus !== 'connected') return;
    if (!serviceOrgId || !/^\d+$/.test(serviceOrgId)) {
      setConnectedServiceOrg(null);
      return;
    }
    const id = parseInt(serviceOrgId, 10);
    if (connectedServiceOrg?.id === id) return;
    const handle = setTimeout(async () => {
      try {
        const info = await api.getServiceOrgInfo(id);
        setConnectedServiceOrg({ id, name: info.name });
      } catch {
        setConnectedServiceOrg({ id, name: `SO ${id}` });
      }
    }, 350);
    return () => clearTimeout(handle);
  }, [serviceOrgId, connectionStatus]);


  const loadProfiles = async () => {
    try {
      const profs = await api.getProfiles();
      setProfiles(profs);
      const active = await api.getActiveProfile();
      if (active) {
        setActiveProfile(active);
        setFqdn(active.source.fqdn);
        setServiceOrgId('');

        try {
          const storedJwt = await api.getCredentials(active.name);
          if (storedJwt) setJwt(storedJwt);
          setApiUsername(active.source.username || '');
        } catch (e) {
          // Ignore error loading creds
        }

        if (active.destination) {
          setDestFqdn(active.destination.fqdn);
          setDestServiceOrgId('');
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

  /** Save a credential and tell the user where it went, warning if it is not in the OS keychain. */
  const saveCredentialsWithNotice = async (name: string, secret: string) => {
    const result = await api.saveCredentials(name, secret);
    if (result.storage === 'keychain') {
      addLog('info', `Credentials for "${name}" saved to the system keychain`);
    } else if (result.storage === 'encryptedFile') {
      addLog('info', `Credentials for "${name}" saved to the encrypted credentials file`);
    } else {
      addLog('warning', `Credentials for "${name}" were not saved and will be forgotten when the app closes`);
    }
    if (result.warning) addLog('warning', result.warning);
  };

  const handleConnect = async () => {
    if (!fqdn.trim() || !jwt.trim()) {
      addLog('error', 'Please enter the server FQDN and the JWT token');
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

        // Don't auto-populate the SO. The chip-sync effect updates the chip
        // once the user picks one from the dropdown.
        if (result.serviceOrgName) {
          addLog('info', `Server's default SO is ${result.serviceOrgName} (ID: ${result.serviceOrgId}). Pick a target SO before continuing.`);
        }

        addLog('success', `Connected to ${result.serverUrl || fqdn}`);
        if (result.serverVersion) addLog('info', `Server version: ${result.serverVersion}`);

        // Keep the saved token fresh, but only for the profile whose server this is: connecting
        // to another server must never overwrite the selected profile's credential.
        if (activeProfile && serverHost(fqdn) === serverHost(activeProfile.source.fqdn)) {
          await saveCredentialsWithNotice(activeProfile.name, jwt);
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

  /** Forget the displayed state of both connections (the backend is cleared by api.disconnect). */
  const resetConnectionState = () => {
    setConnectionStatus('disconnected');
    setServerVersion('');
    setServerUrl('');
    setConnectedServiceOrg(null);
    setDestConnectionStatus('disconnected');
    setDestServerVersion('');
    setDestServerUrl('');
    setDestConnectedServiceOrg(null);
  };

  const handleDisconnect = async () => {
    await api.disconnect();
    resetConnectionState();
    addLog('info', 'Disconnected');
    setCurrentStep('setup');
  };

  const handleSaveProfile = async () => {
    const name = newProfileName.trim();
    if (!name || !fqdn.trim()) {
      addLog('error', 'Please enter profile name and server FQDN');
      return;
    }
    // A profile's destination token is stored as "<name>_dest", so a profile with a name that
    // ends that way would share a credential slot with another profile's destination.
    if (/_dest$/i.test(name)) {
      addLog('error', 'A profile name cannot end with "_dest"');
      return;
    }
    if (profiles.some(p => p.name === name)
      && !window.confirm(`A profile named "${name}" already exists. Replace it and its saved credentials?`)) {
      return;
    }

    try {
      const isMigration = appMode === 'migrate' && destFqdn;
      const profile: Profile = {
        name,
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
      await api.setActiveProfile(name);

      if (jwt) {
        await saveCredentialsWithNotice(name, jwt);
      }
      if (isMigration && destJwt) {
        await saveCredentialsWithNotice(`${name}_dest`, destJwt);
      }

      await loadProfiles();
      setActiveProfile(profile);
      addLog('success', `Profile "${name}" saved`);
      setNewProfileName('');
      setShowNewProfile(false);
    } catch (e) {
      addLog('error', `Failed to save profile: ${e}`);
    }
  };

  // Selecting a profile takes several round trips. If another profile is clicked meanwhile,
  // the earlier selection must stop, or its late results would overwrite the newer one.
  const selectRequest = useRef(0);

  const handleSelectProfile = async (profile: Profile) => {
    const request = ++selectRequest.current;
    const stale = () => request !== selectRequest.current;
    // Drop the previous profile's connections first, in the backend and in the UI, so
    // nothing from the old profile (a live client, a destination, cached lookups) can be
    // used while the new profile is unconnected or fails to connect.
    try {
      await api.disconnect();
    } catch (e) {
      addLog('error', `Failed to disconnect the previous profile: ${e}`);
    }
    if (stale()) return;
    resetConnectionState();

    setActiveProfile(profile);
    setFqdn(profile.source.fqdn);
    setServiceOrgId('');

    // Profiles are mode-agnostic — load source and (if present) destination
    // data without changing appMode. The user picks the mode from Home.
    if (profile.destination) {
      setDestFqdn(profile.destination.fqdn);
      setDestServiceOrgId('');
    } else {
      // The new profile has no destination: forget the previous profile's.
      setDestFqdn('');
      setDestServiceOrgId('');
      setDestJwt('');
      setDestApiUsername('');
    }

    try {
      const storedJwt = await api.getCredentials(profile.name);
      if (stale()) return;
      if (storedJwt) {
        setJwt(storedJwt);
        setApiUsername(profile.source.username || '');
      } else {
        setJwt('');
        setApiUsername('');
      }
    } catch {
      setJwt('');
      setApiUsername('');
    }

    if (profile.destination) {
      try {
        const storedDestJwt = await api.getCredentials(`${profile.name}_dest`);
        if (stale()) return;
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
              if (stale()) return;
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
      if (stale()) return;
      if (hasCreds) {
        setConnectionStatus('connecting');
        addLog('info', `Connecting with saved credentials for ${profile.name}...`);
        const result = await api.connectWithProfile(profile.name, profile.source.fqdn, profile.source.username);
        if (stale()) return;
        if (result.success) {
          setConnectionStatus('connected');
          setServerVersion(result.serverVersion || '');
          setServerUrl(result.serverUrl || profile.source.fqdn);

          if (result.serviceOrgName) {
            addLog('info', `Server's default SO is ${result.serviceOrgName} (ID: ${result.serviceOrgId}). Pick a target SO before continuing.`);
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
      // Installer scripts go in a new folder inside the chosen one; open that one.
      const firstScript = appMode === 'installers' ? lastExportResult?.filesCreated[0] : undefined;
      const directory = firstScript ? firstScript.replace(/[\\/][^\\/]*$/, '') : outputDir;
      if (directory) {
        await api.openDirectory(directory);
        addLog('info', `Opened directory: ${directory}`);
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
    setLastExportResult(null);
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
        deviceAssets: selectedTypes.has('device_assets'),
        registrationTokens: selectedTypes.has('registration_tokens')
      };

      const result = await api.startExport(outputDir, options, Array.from(exportFormats), parseInt(serviceOrgId));
      setLastExportResult(result);
      if (result.cancelled) {
        addLog('warning', result.message);
      } else if (result.partial) {
        addLog('warning', result.message);
      } else if (result.success) {
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

  const handleGenerateInstallers = async () => {
    if (!serviceOrgId) { addLog('error', 'Please enter Service Organization ID'); return; }
    const chosen = installerTargets.filter(t => installerSelected.has(installerTargetKey(t)));
    if (chosen.length === 0) { addLog('error', 'Please select at least one customer or site'); return; }
    // The scripts hold registration tokens in plain text, so confirm before writing them.
    if (!window.confirm('The generated scripts contain registration tokens in plain text. Store and share them securely, and delete them after deployment.\n\nGenerate the scripts?')) return;

    setCurrentStep('exporting');
    setProgress(null);
    setLastExportResult(null);
    addLog('info', `Generating installer scripts for ${chosen.length} customers and sites...`);

    try {
      const result = await api.generateInstallerScripts(outputDir, chosen);
      setLastExportResult(result);
      addLog(result.cancelled || result.partial ? 'warning' : result.success ? 'success' : 'error', result.message);
      for (const w of result.warnings ?? []) addLog('warning', w);
      for (const e of result.errors ?? []) addLog('error', e);
    } catch (e) {
      addLog('error', `Generating installer scripts failed: ${e}`);
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
    const targetLabel = connectedServiceOrg
      ? `${connectedServiceOrg.name} (ID: ${connectedServiceOrg.id})`
      : `SO ${serviceOrgId}`;
    addLog('info', `Starting ${dryRun ? 'dry-run import' : 'live import'}: ${importResource} into ${targetLabel} from ${importCsvPath}`);

    try {
      const result = await api.startImport(
        importResource,
        importCsvPath,
        parseInt(serviceOrgId),
        dryRun,
        importResource === 'users' ? passwordPolicy : undefined,
      );
      setLastImportResult(result);
      addLog(result.cancelled ? 'warning' : result.success ? 'success' : 'warning', result.message);
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
      const options: MigrationOptions = toMigrationOptions(selectedTypes);

      const actualDestSoId = destServiceOrgId ? parseInt(destServiceOrgId) : destConnectedServiceOrg.id;
      addLog('info', `Starting migration: Source SO ${parseInt(serviceOrgId)} → Destination SO ${actualDestSoId}`);

      const result = await api.startMigration(options, parseInt(serviceOrgId), actualDestSoId);
      if (result.cancelled) {
        addLog('warning', result.message);
      } else if (result.success) {
        addLog('success', result.message);
      } else if (result.issues.length > 0) {
        addLog('error', result.message);
      } else {
        addLog('warning', result.message);
      }
      for (const issue of result.issues) addLog('error', issue);
      for (const item of result.skipped) addLog('info', item);
      for (const item of result.manualFollowUp) addLog('warning', item);
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

  // While an export, import, migration or script run is going, leaving the page or dropping the
  // connection would not stop it, and the finished run would pull the user back to the results.
  const running = currentStep === 'exporting';

  // What the current workflow page is for, shown at the top of the page
  const modeNoun = appMode === 'migrate' ? 'Migration' : appMode === 'import' ? 'Import' : appMode === 'installers' ? 'Installer scripts' : 'Export';
  const pageTitle = currentStep === 'setup'
    ? (appMode === 'migrate' ? 'Connect source and destination' : 'Connect to N-central')
    : currentStep === 'configure'
      ? (appMode === 'migrate' ? 'Choose what to migrate'
        : appMode === 'import' ? 'Choose what to import'
        : appMode === 'installers' ? 'Choose customers and sites'
        : 'Choose what to export')
      : modeNoun;
  const pageSubtitle = currentStep === 'setup'
    ? (appMode === 'migrate'
      ? 'Connect both servers with an API token. A saved profile fills in both.'
      : 'Pick a saved profile, or enter the server and an API token.')
    : currentStep === 'configure'
      ? (appMode === 'migrate' ? 'Nothing is changed on the destination until you start the migration.'
        : appMode === 'import' ? 'Run a dry-run first to see what would change.'
        : appMode === 'installers' ? 'One Windows installer script is created for each customer or site you tick.'
        : 'Pick the service org, the data to export and the file format.')
      : undefined;

  return (
    <div className="app">
      {/* Slim header strip — only shown after the home page */}
      {currentStep !== 'home' && currentStep !== 'settings' && (
        <header className="header">
          <div className="header-title">
            <h1
              onClick={() => { if (!running) setCurrentStep('home'); }}
              style={{ cursor: running ? 'default' : 'pointer' }}
              title={running ? undefined : 'Back to Home'}
            >
              <span className="header-accent">N-xport</span>
            </h1>
            <span className="badge" style={{ background: 'var(--color-bg-tertiary)', color: 'var(--color-text-muted)', fontSize: 'var(--text-xs)' }}>v{appVersion}</span>
            <button
              className="btn btn-ghost"
              onClick={() => setCurrentStep('home')}
              disabled={running}
              style={{ padding: '4px 10px', fontSize: 'var(--text-sm)', marginLeft: 8 }}
              title={running ? 'Wait for the run to finish, or cancel it' : 'Back to Home'}
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
                {connectionStatus === 'connected' && !running && (
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
                    else if (appMode === 'installers') void handleGenerateInstallers();
                    else handleExport();
                  }
                }}
                disabled={
                  currentStep === 'setup'
                    ? (appMode === 'migrate' ? (connectionStatus !== 'connected' || destConnectionStatus !== 'connected') : connectionStatus !== 'connected')
                    : (appMode === 'migrate'
                        ? (!serviceOrgId || !destConnectedServiceOrg || !Object.values(toMigrationOptions(selectedTypes)).some(Boolean))
                        : appMode === 'import'
                          ? (!serviceOrgId || !importCsvPath || !importResource)
                          : appMode === 'installers'
                            ? (!serviceOrgId || !outputDir || installerSelected.size === 0)
                          : (!serviceOrgId || !outputDir || selectedTypes.size === 0))
                }
              >
                {currentStep === 'setup'
                  ? <>Continue<span className="kbd">↵</span></>
                  : appMode === 'migrate'
                    ? <>Start migration<span className="kbd">↵</span></>
                    : appMode === 'import'
                      ? (importDryRun ? <>Run dry-run<span className="kbd">↵</span></> : <>Start import<span className="kbd">↵</span></>)
                      : appMode === 'installers'
                        ? <>Generate scripts<span className="kbd">↵</span></>
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
              onSelectExport={() => {
                // Export starts with nothing selected, so one type can be picked without clearing the rest
                setSelectedTypes(new Set());
                setAppMode('export');
                setCurrentStep('setup');
              }}
              onSelectMigrate={() => {
                setSelectedTypes(new Set(exportTypes.filter(t => t.default && !t.exportOnly).map(t => t.id)));
                setAppMode('migrate');
                setCurrentStep('setup');
              }}
              onSelectImport={() => { setAppMode('import'); setCurrentStep('setup'); }}
              onOpenSettings={() => setCurrentStep('settings')}
              onSelectInstallers={() => {
                setInstallerTargets([]);
                setInstallerSelected(new Set());
                setAppMode('installers');
                setCurrentStep('setup');
              }}
            />
          )}

          {currentStep === 'settings' && (
            <SettingsPanel
              addLog={addLog}
              onCredentialsChanged={loadProfiles}
              onBack={() => setCurrentStep('home')}
            />
          )}

          {currentStep !== 'home' && currentStep !== 'settings' && (
            <PageHead
              title={pageTitle}
              subtitle={pageSubtitle}
              step={currentStep === 'setup' ? 'setup' : currentStep === 'configure' ? 'configure' : 'run'}
              runLabel={appMode === 'migrate' ? 'Migrate' : appMode === 'import' ? 'Import' : appMode === 'installers' ? 'Generate' : 'Export'}
              onSetup={() => { if (!running) setCurrentStep('setup'); }}
              onConfigure={() => setCurrentStep('configure')}
            />
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

          {currentStep === 'configure' && appMode === 'installers' && (
            <InstallerPanel
              serviceOrgId={serviceOrgId} setServiceOrgId={setServiceOrgId}
              outputDir={outputDir} setOutputDir={setOutputDir}
              targets={installerTargets} setTargets={setInstallerTargets}
              selected={installerSelected} setSelected={setInstallerSelected}
              onBrowseOutput={handleBrowseOutput}
              onBack={() => setCurrentStep('setup')}
              addLog={addLog}
              connectedServiceOrgName={connectedServiceOrg?.name}
            />
          )}

          {currentStep === 'configure' && appMode !== 'import' && appMode !== 'installers' && (
            <ConfigurePanel
              appMode={appMode as 'export' | 'migrate'}
              serviceOrgId={serviceOrgId} setServiceOrgId={setServiceOrgId}
              outputDir={outputDir} setOutputDir={setOutputDir}
              exportTypes={exportTypes}
              selectedTypes={selectedTypes}
              exportFormats={exportFormats}
              onToggleExportType={toggleExportType}
              onSetSelectedTypes={setSelectedTypes}
              onToggleFormat={toggleFormat}
              onBrowseOutput={handleBrowseOutput}
              onBack={() => setCurrentStep('setup')}
              connectedServiceOrgName={connectedServiceOrg?.name}
              sourceSoId={serviceOrgId ? parseInt(serviceOrgId) : undefined}
              destSoId={destServiceOrgId ? parseInt(destServiceOrgId) : destConnectedServiceOrg?.id}
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
              passwordPolicy={passwordPolicy}
              setPasswordPolicy={setPasswordPolicy}
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
                try {
                  await api.cancelExport();
                } catch (e) {
                  addLog('error', `Failed to request cancellation: ${e}`);
                }
              }}
              lastImportResult={lastImportResult}
              lastExportResult={lastExportResult}
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
