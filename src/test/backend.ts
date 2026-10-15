import { mockIPC } from '@tauri-apps/api/mocks';
import type { ConnectionResult, ExportType, ImportType, Profile } from '../types';

export interface RecordedCall {
  cmd: string;
  args: Record<string, unknown>;
}

type Handler = (args: Record<string, unknown>) => unknown | Promise<unknown>;

export const EXPORT_TYPES: ExportType[] = [
  { id: 'customers', name: 'Customers', default: true },
  { id: 'sites', name: 'Sites', default: true },
  { id: 'users', name: 'Users', default: true },
  { id: 'user_roles', name: 'User Roles', default: false },
  { id: 'access_groups', name: 'Access Groups', default: false },
  { id: 'org_properties', name: 'Organization Properties', default: false },
  { id: 'device_properties', name: 'Device Properties', default: false },
  { id: 'registration_tokens', name: 'Registration Tokens', default: false, exportOnly: true },
];

export const IMPORT_TYPES: ImportType[] = [
  { id: 'customers', name: 'Customers', supported: true },
  { id: 'sites', name: 'Sites', supported: true },
  { id: 'users', name: 'Users', supported: true },
  { id: 'org_properties', name: 'Organization Properties', supported: true },
  { id: 'device_properties', name: 'Device Properties', supported: true },
  { id: 'devices', name: 'Devices', supported: false },
];

export const OK_CONNECTION: ConnectionResult = {
  success: true,
  message: 'Connection successful',
  serverUrl: 'https://nc.example.com',
  serverVersion: '2025.1',
  serviceOrgId: 50,
  serviceOrgName: 'Main SO',
};

export interface Backend {
  /** Every command the frontend invoked, in order. */
  calls: RecordedCall[];
  /** Commands invoked with this name, in order. */
  callsTo(cmd: string): RecordedCall[];
  /** Names of every command invoked, in order. */
  sequence(): string[];
}

/**
 * Mock the Tauri backend for one test. `handlers` overrides or adds commands; anything not
 * handled returns null, and every call is recorded so a test can assert what the UI asked for.
 */
export function mockBackend(handlers: Record<string, Handler> = {}): Backend {
  const calls: RecordedCall[] = [];
  const profiles: Profile[] = [];
  const defaults: Record<string, Handler> = {
    get_profiles: () => profiles,
    get_active_profile: () => null,
    get_settings: () => ({ profiles: [], exportFormats: ['csv'] }),
    get_export_types: () => EXPORT_TYPES,
    get_import_types: () => IMPORT_TYPES,
    list_service_orgs: () => [{ id: 50, name: 'Main SO' }],
    get_service_org_info: (args) => ({ id: args.serviceOrgId, name: `SO ${args.serviceOrgId}` }),
    has_credentials: () => false,
    get_credentials: () => null,
    get_password: () => null,
    disconnect: () => null,
    'plugin:app|version': () => '1.2.0',
    'plugin:updater|check': () => null,
    ...handlers,
  };
  mockIPC(
    async (cmd, payload) => {
      const args = (payload ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args });
      const handler = defaults[cmd];
      return handler ? handler(args) : null;
    },
    { shouldMockEvents: true },
  );
  return {
    calls,
    callsTo: (cmd) => calls.filter((call) => call.cmd === cmd),
    sequence: () => calls.map((call) => call.cmd),
  };
}
