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
}

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
}

export interface ExportResult {
  success: boolean;
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
}

export type ConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'error';

export interface MigrationOptions {
  customers: boolean;
  userRoles: boolean;
  accessGroups: boolean;
  users: boolean;
  orgProperties: boolean;
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
