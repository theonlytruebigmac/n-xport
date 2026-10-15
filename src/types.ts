// TypeScript types for N-Central Data Export Tool

export type ProfileType = 'export' | 'migration';

export interface ConnectionConfig {
  fqdn: string;
  username?: string;
  serviceOrgId?: number;
}

export interface Profile {
  name: string;
  type: ProfileType;
  source: ConnectionConfig;
  destination?: ConnectionConfig;
  lastUsed?: string;
}

export interface Settings {
  profiles: Profile[];
  activeProfile?: string;
  exportDirectory?: string;
  exportFormats: string[];
  window: WindowState;
  passwordPolicy?: PasswordPolicy;
}

export interface PasswordPolicy {
  minLength: number;
  minDigits: number;
  minSpecial: number;
  minUppercase: number;
  minLowercase: number;
}

export const DEFAULT_PASSWORD_POLICY: PasswordPolicy = {
  minLength: 8,
  minDigits: 1,
  minSpecial: 3,
  minUppercase: 1,
  minLowercase: 1,
};

export interface WindowState {
  width: number;
  height: number;
  x?: number;
  y?: number;
  maximized: boolean;
}

export interface ConnectionResult {
  success: boolean;
  message: string;
  serverUrl?: string;
  serverVersion?: string;
  serviceOrgId?: number;
  serviceOrgName?: string;
}

export interface ServiceOrg {
  id: number;
  name: string;
}

export interface ExportOptions {
  serviceOrgs: boolean;
  customers: boolean;
  sites: boolean;
  devices: boolean;
  accessGroups: boolean;
  userRoles: boolean;
  orgProperties: boolean;
  deviceProperties: boolean;
  users: boolean;
  deviceAssets: boolean;
}

export interface ExportResult {
  success: boolean;
  cancelled: boolean;
  partial: boolean;
  message: string;
  filesCreated: string[];
  totalRecords: number;
  warnings: string[];
  errors: string[];
}

export interface ProgressUpdate {
  phase: string;
  message: string;
  percent: number;
  current: number;
  total: number;
}

export interface ExportType {
  id: string;
  name: string;
  default: boolean;
  migrationComingSoon?: boolean;
}

export type ConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'error';

export interface MigrationOptions {
  customers: boolean;
  userRoles: boolean;
  accessGroups: boolean;
  users: boolean;
  orgProperties: boolean;
  deviceProperties: boolean;
}

export type StorageBackend = 'keychain' | 'encryptedFile' | 'sessionOnly';

export interface SaveCredentialsResult {
  /** Where the JWT was stored: the OS keychain, the encrypted file, or nowhere (this session only). */
  storage: StorageBackend;
  /** Set when the user needs to know something, such as the credential not being saved. */
  warning: string | null;
}

/** The opt-in encrypted credential file, as the backend reports it. */
export interface CredentialStorageStatus {
  /** The user has turned on encrypted file storage. */
  diskEnabled: boolean;
  /** An encrypted file exists on disk. */
  vaultExists: boolean;
  /** The passphrase has been entered this session. */
  unlocked: boolean;
  /** Credentials still in the old, only lightly obfuscated file. */
  legacyEntries: number;
}

export interface LegacyMigration {
  toKeychain: number;
  toEncryptedFile: number;
  remaining: number;
}

/** Shortest passphrase accepted for a new encrypted credential file. */
export const MIN_PASSPHRASE_LENGTH = 12;

export type SoapUse = 'none' | 'fallback' | 'preferred' | 'required';

export interface MigrationPreflightStep {
  resource: string;
  label: string;
  soap: SoapUse;
  details: string;
}

export interface MigrationPreflight {
  sourceSoId: number;
  destSoId: number;
  steps: MigrationPreflightStep[];
  usesSoap: boolean;
  /** Things that must already be true on the destination before the run starts. */
  prerequisites: string[];
  warnings: string[];
  manualFollowUp: string[];
}

/** Map the selected data-type ids to the options the backend expects. */
export function toMigrationOptions(selected: Set<string>): MigrationOptions {
  return {
    customers: selected.has('customers'),
    userRoles: selected.has('user_roles'),
    accessGroups: selected.has('access_groups'),
    users: selected.has('users'),
    orgProperties: selected.has('org_properties'),
    deviceProperties: selected.has('device_properties'),
  };
}

export interface ImportType {
  id: string;
  name: string;
  supported: boolean;
}

export type ImportRowStatus = 'created' | 'updated' | 'skipped' | 'error' | 'planned';

export interface ImportRowOutcome {
  rowNumber: number;
  status: ImportRowStatus;
  label: string;
  message: string;
}

export interface ImportResult {
  success: boolean;
  cancelled: boolean;
  message: string;
  dryRun: boolean;
  rowsTotal: number;
  rowsCreated: number;
  rowsUpdated: number;
  rowsSkipped: number;
  rowsErrored: number;
  rowsPlanned: number;
  outcomes: ImportRowOutcome[];
}

export interface MigrationResult {
  success: boolean;
  cancelled: boolean;
  issues: string[];
  skipped: string[];
  manualFollowUp: string[];
  message: string;
}

export interface AppState {
  connectionStatus: ConnectionStatus;
  serverVersion?: string;
  activeProfile?: Profile;
  exportProgress?: ProgressUpdate;
  logs: LogEntry[];
}

export interface LogEntry {
  timestamp: Date;
  level: 'info' | 'success' | 'warning' | 'error' | 'debug';
  message: string;
}
