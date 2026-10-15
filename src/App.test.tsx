import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { emit } from '@tauri-apps/api/event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import { mockBackend, OK_CONNECTION } from './test/backend';
import type { ExportResult, ImportResult, Profile } from './types';

const FAILED_CONNECTION = { success: false, message: 'Authentication failed: bad token' };

const exportResult = (overrides: Partial<ExportResult> = {}): ExportResult => ({
  success: true,
  cancelled: false,
  partial: false,
  message: 'Exported 3 records to 1 files',
  filesCreated: ['customers.csv'],
  totalRecords: 3,
  warnings: [],
  errors: [],
  ...overrides,
});

const importResult = (overrides: Partial<ImportResult> = {}): ImportResult => ({
  success: true,
  cancelled: false,
  message: 'done',
  dryRun: true,
  rowsTotal: 2,
  rowsCreated: 0,
  rowsUpdated: 0,
  rowsSkipped: 0,
  rowsErrored: 0,
  rowsPlanned: 2,
  outcomes: [],
  ...overrides,
});

const sourceInputs = () => ({
  fqdn: screen.getAllByPlaceholderText('ncentral.example.com')[0],
  username: screen.getAllByPlaceholderText('admin@example.com')[0],
  jwt: screen.getAllByPlaceholderText(/^eyJ/)[0],
});

/** Pick a mode on the home screen, fill in the source connection, and connect. */
async function connectSource(user: ReturnType<typeof userEvent.setup>, mode: string) {
  await user.click(await screen.findByText(mode));
  const { fqdn, username, jwt } = sourceInputs();
  await user.type(fqdn, 'nc.example.com');
  await user.type(username, 'api@example.com');
  await user.type(jwt, 'secret-jwt');
  await user.click(screen.getByRole('button', { name: /^Connect/ }));
}

const continueButton = () => screen.getByRole('button', { name: /^Continue/ });

/** Type a numeric service-org ID into the service org picker on the configure screen. */
async function pickServiceOrg(user: ReturnType<typeof userEvent.setup>) {
  await user.type(await screen.findByPlaceholderText('Select or type a service org…'), '50');
}

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

describe('connection', () => {
  it('connects with the entered server, token and API username', async () => {
    const backend = mockBackend({ test_connection: () => OK_CONNECTION });
    const user = userEvent.setup();
    render(<App />);

    await connectSource(user, 'Export Data');

    await waitFor(() => expect(continueButton()).toBeEnabled());
    expect(backend.callsTo('test_connection')[0].args).toEqual({
      fqdn: 'nc.example.com',
      jwt: 'secret-jwt',
      username: 'api@example.com',
    });
  });

  it('does not let a failed connection continue', async () => {
    const backend = mockBackend({ test_connection: () => FAILED_CONNECTION });
    const user = userEvent.setup();
    render(<App />);

    await connectSource(user, 'Export Data');

    await waitFor(() => expect(backend.callsTo('test_connection')).toHaveLength(1));
    await waitFor(() => expect(screen.getByRole('button', { name: /^Connect/ })).toBeEnabled());
    expect(continueButton()).toBeDisabled();
  });
});

describe('export', () => {
  it('exports exactly the selected types and formats', async () => {
    const backend = mockBackend({
      test_connection: () => OK_CONNECTION,
      start_export: () => exportResult(),
    });
    const user = userEvent.setup();
    render(<App />);

    await connectSource(user, 'Export Data');
    await waitFor(() => expect(continueButton()).toBeEnabled());
    await user.click(continueButton());
    await pickServiceOrg(user);

    // Nothing is selected to start with. Pick customers, users and user roles, and add JSON.
    await user.click(await screen.findByRole('checkbox', { name: 'Customers' }));
    await user.click(screen.getByRole('checkbox', { name: 'Users' }));
    await user.click(screen.getByRole('checkbox', { name: 'User Roles' }));
    await user.click(screen.getByRole('checkbox', { name: 'json' }));
    await user.click(screen.getByRole('button', { name: /^Start export/ }));

    await waitFor(() => expect(backend.callsTo('start_export')).toHaveLength(1));
    const args = backend.callsTo('start_export')[0].args as {
      options: Record<string, boolean>;
      formats: string[];
      serviceOrgId: number;
    };
    expect(args.serviceOrgId).toBe(50);
    expect([...args.formats].sort()).toEqual(['csv', 'json']);
    expect(args.options).toMatchObject({
      customers: true,
      sites: false,
      users: true,
      userRoles: true,
      accessGroups: false,
    });
    expect(await screen.findByText('Export complete')).toBeInTheDocument();
  });

  it('offers registration tokens for export, and sends the choice to the backend', async () => {
    const backend = mockBackend({
      test_connection: () => OK_CONNECTION,
      start_export: () => exportResult(),
    });
    const user = userEvent.setup();
    render(<App />);

    await connectSource(user, 'Export Data');
    await waitFor(() => expect(continueButton()).toBeEnabled());
    await user.click(continueButton());
    await pickServiceOrg(user);

    // Only registration tokens: nothing else needs unticking first.
    const tokens = await screen.findByRole('checkbox', { name: 'Registration Tokens' });
    expect(tokens).not.toBeChecked();
    await user.click(tokens);
    await user.click(screen.getByRole('button', { name: /^Start export/ }));

    await waitFor(() => expect(backend.callsTo('start_export')).toHaveLength(1));
    expect(backend.callsTo('start_export')[0].args).toMatchObject({
      options: { registrationTokens: true, customers: false, sites: false, users: false },
    });
  });

  it('starts with nothing selected and cannot start until something is', async () => {
    mockBackend({ test_connection: () => OK_CONNECTION });
    const user = userEvent.setup();
    render(<App />);

    await connectSource(user, 'Export Data');
    await waitFor(() => expect(continueButton()).toBeEnabled());
    await user.click(continueButton());
    await pickServiceOrg(user);

    const startButton = () => screen.getByRole('button', { name: /^Start export/ });
    await screen.findByRole('checkbox', { name: 'Customers' });
    expect(startButton()).toBeDisabled();

    await user.click(screen.getByRole('button', { name: 'Select all' }));
    expect(screen.getByRole('checkbox', { name: 'Registration Tokens' })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'Device Properties' })).toBeChecked();
    expect(startButton()).toBeEnabled();

    await user.click(screen.getByRole('button', { name: 'Clear all' }));
    expect(screen.getByRole('checkbox', { name: 'Customers' })).not.toBeChecked();
    expect(startButton()).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Select all' })).toBeInTheDocument();
  });

  it('shows a partial export as partial, with its warnings', async () => {
    mockBackend({
      test_connection: () => OK_CONNECTION,
      start_export: () =>
        exportResult({
          partial: true,
          message: 'Partial export: wrote 3 records to 1 files with 1 warning(s)',
          warnings: ['Failed to fetch Users for OU 12: timeout'],
        }),
    });
    const user = userEvent.setup();
    render(<App />);

    await connectSource(user, 'Export Data');
    await waitFor(() => expect(continueButton()).toBeEnabled());
    await user.click(continueButton());
    await pickServiceOrg(user);
    await user.click(await screen.findByRole('checkbox', { name: 'Customers' }));
    await user.click(await screen.findByRole('button', { name: /^Start export/ }));

    expect(await screen.findByText('Partial export')).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('Failed to fetch Users for OU 12: timeout');
  });
});

describe('while a run is going', () => {
  it('cannot be left or disconnected until it finishes', async () => {
    let finish: (value: unknown) => void = () => {};
    mockBackend({
      test_connection: () => OK_CONNECTION,
      start_export: () => new Promise((resolve) => { finish = resolve; }),
    });
    const user = userEvent.setup();
    render(<App />);

    await connectSource(user, 'Export Data');
    await waitFor(() => expect(continueButton()).toBeEnabled());
    await user.click(continueButton());
    await pickServiceOrg(user);
    await user.click(await screen.findByRole('checkbox', { name: 'Customers' }));
    await user.click(screen.getByRole('button', { name: /^Start export/ }));

    const home = await screen.findByRole('button', { name: /Home/ });
    expect(home).toBeDisabled();
    expect(screen.queryByTitle('Disconnect')).not.toBeInTheDocument();

    finish(exportResult());
    expect(await screen.findByText('Export complete')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Home/ })).toBeEnabled();
  });
});

describe('output folder', () => {
  it('defaults to a folder inside Downloads rather than a path relative to the app', async () => {
    const backend = mockBackend({
      test_connection: () => OK_CONNECTION,
      'plugin:path|resolve_directory': () => '/Users/me/Downloads/',
      start_export: () => exportResult(),
    });
    const user = userEvent.setup();
    render(<App />);

    await connectSource(user, 'Export Data');
    await waitFor(() => expect(continueButton()).toBeEnabled());
    await user.click(continueButton());
    await pickServiceOrg(user);
    await user.click(await screen.findByRole('checkbox', { name: 'Customers' }));
    await waitFor(() => expect(screen.getByDisplayValue('/Users/me/Downloads/nc_export')).toBeInTheDocument());
    await user.click(screen.getByRole('button', { name: /^Start export/ }));

    await waitFor(() => expect(backend.callsTo('start_export')).toHaveLength(1));
    expect(backend.callsTo('start_export')[0].args).toMatchObject({ outputDir: '/Users/me/Downloads/nc_export' });
  });

  it('remembers the folder that was typed for next time', async () => {
    mockBackend({ test_connection: () => OK_CONNECTION });
    const user = userEvent.setup();
    const { unmount } = render(<App />);

    await connectSource(user, 'Export Data');
    await waitFor(() => expect(continueButton()).toBeEnabled());
    await user.click(continueButton());
    const folder = await screen.findByDisplayValue('../nc_export');
    await user.clear(folder);
    await user.type(folder, '/data/exports');
    unmount();

    mockBackend({ test_connection: () => OK_CONNECTION });
    render(<App />);
    await connectSource(user, 'Export Data');
    await waitFor(() => expect(continueButton()).toBeEnabled());
    await user.click(continueButton());
    expect(await screen.findByDisplayValue('/data/exports')).toBeInTheDocument();
  });
});

describe('cancellation', () => {
  it('asks the backend to cancel and shows the cancelled result', async () => {
    let finishExport: (result: ExportResult) => void = () => {};
    const backend = mockBackend({
      test_connection: () => OK_CONNECTION,
      start_export: () => new Promise<ExportResult>((resolve) => { finishExport = resolve; }),
    });
    const user = userEvent.setup();
    render(<App />);

    await connectSource(user, 'Export Data');
    await waitFor(() => expect(continueButton()).toBeEnabled());
    await user.click(continueButton());
    await pickServiceOrg(user);
    await user.click(await screen.findByRole('checkbox', { name: 'Customers' }));
    await user.click(await screen.findByRole('button', { name: /^Start export/ }));

    // The Cancel button appears once the backend reports progress.
    await waitFor(() => expect(backend.callsTo('start_export')).toHaveLength(1));
    await act(async () => {
      await emit('export-progress', { phase: 'Customers', message: 'Fetching', percent: 10, current: 1, total: 10 });
    });
    await user.click(await screen.findByRole('button', { name: 'Cancel' }));
    expect(backend.callsTo('cancel_export')).toHaveLength(1);

    await act(async () => {
      finishExport(exportResult({ success: false, cancelled: true, message: 'Export cancelled after writing 1 records' }));
    });
    expect(await screen.findByRole('heading', { name: 'Cancelled' })).toBeInTheDocument();
  });
});

describe('import', () => {
  async function openImport(user: ReturnType<typeof userEvent.setup>) {
    await connectSource(user, 'Import Data');
    await waitFor(() => expect(continueButton()).toBeEnabled());
    await user.click(continueButton());
    await pickServiceOrg(user);
  }

  it('runs a dry-run first and applies for real only after it', async () => {
    const backend = mockBackend({
      test_connection: () => OK_CONNECTION,
      start_import: (args) =>
        importResult(
          args.dryRun
            ? { dryRun: true, rowsPlanned: 2 }
            : { dryRun: false, rowsPlanned: 0, rowsCreated: 2 },
        ),
    });
    const user = userEvent.setup();
    render(<App />);

    await openImport(user);
    await user.type(await screen.findByPlaceholderText('Choose a CSV file…'), '/tmp/customers.csv');
    await user.click(screen.getByRole('button', { name: /^Run dry-run/ }));

    await waitFor(() => expect(backend.callsTo('start_import')).toHaveLength(1));
    expect(backend.callsTo('start_import')[0].args).toMatchObject({
      resourceType: 'customers',
      csvPath: '/tmp/customers.csv',
      serviceOrgId: 50,
      dryRun: true,
    });
    expect(await screen.findByRole('heading', { name: 'Dry-run complete' })).toBeInTheDocument();
    expect(screen.getByText('Would apply')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /Apply for real/ }));
    await waitFor(() => expect(backend.callsTo('start_import')).toHaveLength(2));
    expect(backend.callsTo('start_import')[1].args).toMatchObject({ dryRun: false, csvPath: '/tmp/customers.csv' });
    expect(await screen.findByRole('heading', { name: 'Complete' })).toBeInTheDocument();
  });

  it('does not offer to apply a dry-run that planned nothing', async () => {
    mockBackend({
      test_connection: () => OK_CONNECTION,
      start_import: () => importResult({ rowsPlanned: 0, rowsSkipped: 2 }),
    });
    const user = userEvent.setup();
    render(<App />);

    await openImport(user);
    await user.type(await screen.findByPlaceholderText('Choose a CSV file…'), '/tmp/customers.csv');
    await user.click(screen.getByRole('button', { name: /^Run dry-run/ }));

    expect(await screen.findByRole('heading', { name: 'Dry-run complete' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Apply for real/ })).not.toBeInTheDocument();
  });

  it('says property definitions and devices must already exist, for the property resources only', async () => {
    mockBackend({ test_connection: () => OK_CONNECTION });
    const user = userEvent.setup();
    render(<App />);

    await openImport(user);
    const notice = () => screen.queryByText('Create these in N-central first.');

    await screen.findByText('Customers');
    expect(notice()).not.toBeInTheDocument();
    await user.click(screen.getByText('Organization Properties'));
    expect(notice()).toBeInTheDocument();
    expect(notice()?.parentElement).toHaveTextContent('definition must already exist');
    expect(notice()?.parentElement).not.toHaveTextContent('device must already be on this server');
    await user.click(screen.getByText('Device Properties'));
    expect(notice()?.parentElement).toHaveTextContent('device must already be on this server');
    await user.click(screen.getByText('Sites'));
    expect(notice()).not.toBeInTheDocument();
  });

  it('shows the SOAP notice only for resources that can call SOAP', async () => {
    mockBackend({ test_connection: () => OK_CONNECTION });
    const user = userEvent.setup();
    render(<App />);

    await openImport(user);
    const notice = () => screen.queryByText('SOAP API may be required.');

    await screen.findByText('Customers');
    expect(notice()).toBeInTheDocument(); // customers: SOAP fallback
    await user.click(screen.getByText('Sites'));
    expect(notice()).not.toBeInTheDocument(); // sites: REST only
    await user.click(screen.getByText('Users'));
    expect(notice()).toBeInTheDocument(); // users: SOAP required
    await user.click(screen.getByText('Device Properties'));
    expect(notice()).not.toBeInTheDocument(); // device properties: REST only
  });
});

describe('profiles', () => {
  const migrationProfile: Profile = {
    name: 'Prod to DR',
    type: 'migration',
    source: { fqdn: 'prod.example.com', username: 'prod-api' },
    destination: { fqdn: 'dr.example.com', username: 'dr-api' },
  };
  const exportProfile: Profile = {
    name: 'Lab',
    type: 'export',
    source: { fqdn: 'lab.example.com', username: 'lab-api' },
  };

  function profileBackend(overrides: Parameters<typeof mockBackend>[0] = {}) {
    return mockBackend({
      get_profiles: () => [migrationProfile, exportProfile],
      has_credentials: () => true,
      get_credentials: (args) => `jwt-for-${args.profileName}`,
      connect_with_profile: (args) =>
        args.profileName === 'Prod to DR' ? OK_CONNECTION : FAILED_CONNECTION,
      connect_destination: () => OK_CONNECTION,
      ...overrides,
    });
  }

  it('disconnects the previous profile first and reconnects with the profile username', async () => {
    const backend = profileBackend();
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByText('Migrate Data'));
    await user.click(await screen.findByText('Prod to DR'));

    await waitFor(() => expect(backend.callsTo('connect_destination')).toHaveLength(1));
    const sequence = backend.sequence();
    expect(sequence.indexOf('disconnect')).toBeGreaterThan(-1);
    expect(sequence.indexOf('disconnect')).toBeLessThan(sequence.indexOf('connect_with_profile'));
    expect(backend.callsTo('connect_with_profile')[0].args).toEqual({
      profileName: 'Prod to DR',
      fqdn: 'prod.example.com',
      username: 'prod-api',
    });
    expect(backend.callsTo('connect_destination')[0].args).toEqual({
      fqdn: 'dr.example.com',
      jwt: 'jwt-for-Prod to DR_dest',
      username: 'dr-api',
    });
    await waitFor(() => expect(continueButton()).toBeEnabled());
  });

  it('forgets the previous destination and connection when switching to another profile', async () => {
    const backend = profileBackend();
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByText('Migrate Data'));
    await user.click(await screen.findByText('Prod to DR'));
    await waitFor(() => expect(continueButton()).toBeEnabled());
    const [, destFqdn] = screen.getAllByPlaceholderText('ncentral.example.com');
    expect(destFqdn).toHaveValue('dr.example.com');

    // Lab has no destination and its connection fails.
    await user.click(screen.getByText('Lab'));

    await waitFor(() => expect(backend.callsTo('connect_with_profile')).toHaveLength(2));
    expect(backend.callsTo('disconnect').length).toBeGreaterThanOrEqual(2);
    expect(screen.getAllByPlaceholderText('ncentral.example.com')[1]).toHaveValue('');
    expect(screen.getAllByPlaceholderText('ncentral.example.com')[0]).toHaveValue('lab.example.com');
    await waitFor(() => expect(continueButton()).toBeDisabled());
    expect(screen.queryAllByText('Connected')).toHaveLength(0);
  });

  async function selectLabAndGetFields(user: ReturnType<typeof userEvent.setup>) {
    await user.click(await screen.findByText('Export Data'));
    await user.click(await screen.findByText('Lab'));
    await waitFor(() => expect(continueButton()).toBeEnabled());
    return sourceInputs();
  }

  it('does not overwrite the selected profile\'s saved token when connecting to a different server', async () => {
    const backend = profileBackend({
      connect_with_profile: () => OK_CONNECTION,
      test_connection: () => OK_CONNECTION,
      save_credentials: () => ({ storage: 'keychain', warning: null }),
    });
    const user = userEvent.setup();
    render(<App />);
    const { fqdn, jwt } = await selectLabAndGetFields(user);

    // Another server and token typed over the profile's, then connect.
    await user.clear(fqdn);
    await user.type(fqdn, 'other.example.com');
    await user.clear(jwt);
    await user.type(jwt, 'a-different-token');
    await user.click(screen.getByRole('button', { name: /^Reconnect/ }));
    await waitFor(() => expect(backend.callsTo('test_connection')).toHaveLength(1));
    expect(backend.callsTo('save_credentials')).toHaveLength(0);
  });

  it('refreshes the selected profile\'s saved token when reconnecting to its own server', async () => {
    const backend = profileBackend({
      connect_with_profile: () => OK_CONNECTION,
      test_connection: () => OK_CONNECTION,
      save_credentials: () => ({ storage: 'keychain', warning: null }),
    });
    const user = userEvent.setup();
    render(<App />);
    const { jwt } = await selectLabAndGetFields(user);

    await user.clear(jwt);
    await user.type(jwt, 'a-fresh-token');
    await user.click(screen.getByRole('button', { name: /^Reconnect/ }));
    await waitFor(() => expect(backend.callsTo('save_credentials')).toHaveLength(1));
    expect(backend.callsTo('save_credentials')[0].args).toMatchObject({
      profileName: 'Lab',
      jwt: 'a-fresh-token',
    });
  });

  it('clears the form when the active profile is deleted', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    let profilesNow: Profile[] = [migrationProfile, exportProfile];
    profileBackend({
      get_profiles: () => profilesNow,
      get_active_profile: () => profilesNow.find((p) => p.name === 'Lab') ?? null,
      connect_with_profile: () => OK_CONNECTION,
      delete_profile: () => { profilesNow = profilesNow.filter((p) => p.name !== 'Lab'); },
    });
    const user = userEvent.setup();
    render(<App />);
    const { fqdn, jwt } = await selectLabAndGetFields(user);
    expect(fqdn).toHaveValue('lab.example.com');
    expect(jwt).toHaveValue('jwt-for-Lab');

    const row = screen.getByText('Lab').closest('.item') as HTMLElement;
    await user.click(within(row).getByTitle('Delete profile'));

    await waitFor(() => expect(screen.queryByText('Lab')).not.toBeInTheDocument());
    expect(sourceInputs().fqdn).toHaveValue('');
    expect(sourceInputs().jwt).toHaveValue('');
  });

  it('keeps the profile when its credentials cannot be deleted', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    const backend = profileBackend({
      delete_credentials: () => { throw new Error('keychain locked'); },
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByText('Export Data'));
    const row = (await screen.findByText('Lab')).closest('.item') as HTMLElement;
    await user.click(within(row).getByTitle('Delete profile'));

    await waitFor(() => expect(backend.callsTo('delete_credentials')).toHaveLength(1));
    expect(backend.callsTo('delete_profile')).toHaveLength(0);
    expect(screen.getByText('Lab')).toBeInTheDocument();
  });
});

describe('settings', () => {
  const status = { diskEnabled: false, vaultExists: false, unlocked: false, legacyEntries: 2 };

  it('keeps credential storage on a settings page opened from home, not on the connection screen', async () => {
    mockBackend({ get_credential_storage_status: () => status });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole('button', { name: 'Settings' }));
    expect(await screen.findByRole('heading', { name: 'Settings' })).toBeInTheDocument();
    expect(await screen.findByLabelText('Credential storage')).toBeInTheDocument();
    expect(screen.queryByText('Export Data')).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /Home/ }));
    await user.click(await screen.findByText('Export Data'));
    await screen.findByText('Profiles');
    expect(screen.queryByLabelText('Credential storage')).not.toBeInTheDocument();
  });
});

describe('import templates', () => {
  it('can be downloaded from the setup page before connecting', async () => {
    const backend = mockBackend({
      'plugin:dialog|save': () => '/tmp/users_template.csv',
      generate_template: (args) => String(args.path),
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByText('Import Data'));
    await user.click(await screen.findByRole('button', { name: /CSV templates/ }));
    await user.click(await screen.findByRole('button', { name: /^Users/ }));

    await waitFor(() => expect(backend.callsTo('generate_template')).toHaveLength(1));
    expect(backend.callsTo('generate_template')[0].args).toMatchObject({
      resourceType: 'users',
      path: '/tmp/users_template.csv',
    });
    expect(backend.callsTo('test_connection')).toHaveLength(0);
  });

  it('is not offered on the export or migrate setup pages', async () => {
    mockBackend();
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByText('Export Data'));
    await screen.findByText('Profiles');
    expect(screen.queryByRole('button', { name: /CSV templates/ })).not.toBeInTheDocument();
  });
});

describe('migration data types', () => {
  it('does not offer export-only types such as registration tokens', async () => {
    mockBackend({ test_connection: () => OK_CONNECTION, connect_destination: () => OK_CONNECTION });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByText('Migrate Data'));
    const type = async (index: number, host: string) => {
      await user.type(screen.getAllByPlaceholderText('ncentral.example.com')[index], host);
      await user.type(screen.getAllByPlaceholderText('admin@example.com')[index], 'api@example.com');
      await user.type(screen.getAllByPlaceholderText(/^eyJ/)[index], 'secret-jwt');
    };
    await type(0, 'source.example.com');
    await user.click(screen.getByRole('button', { name: /^Connect Source/ }));
    await type(1, 'dest.example.com');
    await user.click(screen.getByRole('button', { name: /^Connect Destination/ }));
    await waitFor(() => expect(continueButton()).toBeEnabled());
    await user.click(continueButton());

    expect(await screen.findByRole('checkbox', { name: 'Customers' })).toBeInTheDocument();
    expect(screen.queryByRole('checkbox', { name: 'Registration Tokens' })).not.toBeInTheDocument();
  });
});

describe('migration service orgs', () => {
  it('lists the destination picker from the destination server and the source picker from the source', async () => {
    const backend = mockBackend({
      test_connection: () => OK_CONNECTION,
      connect_destination: () => ({ ...OK_CONNECTION, serviceOrgId: 60, serviceOrgName: 'Dest SO' }),
      list_service_orgs: (args) =>
        args.destination ? [{ id: 1059, name: 'Destination Org' }] : [{ id: 913, name: 'Source Org' }],
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByText('Migrate Data'));
    const type = async (index: number, host: string) => {
      await user.type(screen.getAllByPlaceholderText('ncentral.example.com')[index], host);
      await user.type(screen.getAllByPlaceholderText('admin@example.com')[index], 'api@example.com');
      await user.type(screen.getAllByPlaceholderText(/^eyJ/)[index], 'secret-jwt');
    };
    await type(0, 'source.example.com');
    await user.click(screen.getByRole('button', { name: /^Connect Source/ }));
    await type(1, 'dest.example.com');
    await user.click(screen.getByRole('button', { name: /^Connect Destination/ }));

    await waitFor(() => {
      const flags = backend.callsTo('list_service_orgs').map((call) => Boolean(call.args.destination));
      expect(flags).toContain(true);
      expect(flags).toContain(false);
    });
  });
});

describe('generate installer scripts', () => {
  const targets = [
    { orgUnitId: 10, orgUnitType: 'customer', name: 'Acme', customerName: null, customerId: null },
    { orgUnitId: 11, orgUnitType: 'site', name: 'Acme HQ', customerName: 'Acme', customerId: 10 },
    { orgUnitId: 12, orgUnitType: 'site', name: 'Acme Depot', customerName: 'Acme', customerId: 10 },
    { orgUnitId: 20, orgUnitType: 'customer', name: 'Zeta Co', customerName: null, customerId: null },
    { orgUnitId: 99, orgUnitType: 'site', name: 'Stray Site', customerName: null, customerId: null },
  ];

  async function openConfigure(user: ReturnType<typeof userEvent.setup>) {
    await user.click(await screen.findByRole('button', { name: /Generate Installer Scripts/ }));
    const { fqdn, username, jwt } = sourceInputs();
    await user.type(fqdn, 'nc.example.com');
    await user.type(username, 'api@example.com');
    await user.type(jwt, 'secret-jwt');
    await user.click(screen.getByRole('button', { name: /^Connect/ }));
    await waitFor(() => expect(continueButton()).toBeEnabled());
    await user.click(continueButton());
    await pickServiceOrg(user);
    await screen.findByRole('checkbox', { name: 'Acme - Acme HQ, site, ID 11' });
  }

  it('lists customers and sites, and generates a script for each one selected', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    const backend = mockBackend({
      test_connection: () => OK_CONNECTION,
      list_installer_targets: () => targets,
      generate_installer_scripts: () =>
        exportResult({ message: 'Created 2 script(s)', filesCreated: ['/out/Acme_10.ps1', '/out/Acme - Acme HQ_11.ps1'] }),
    });
    const user = userEvent.setup();
    render(<App />);
    await openConfigure(user);

    const generate = () => screen.getByRole('button', { name: /^Generate scripts/ });
    expect(generate()).toBeDisabled();
    expect(backend.callsTo('list_installer_targets')[0].args).toMatchObject({ serviceOrgId: 50 });

    await user.click(screen.getByRole('checkbox', { name: 'Acme, customer, ID 10' }));
    await user.click(screen.getByRole('checkbox', { name: 'Acme - Acme HQ, site, ID 11' }));
    expect(generate()).toBeEnabled();
    await user.click(generate());

    await waitFor(() => expect(backend.callsTo('generate_installer_scripts')).toHaveLength(1));
    const sent = backend.callsTo('generate_installer_scripts')[0].args as { targets: Array<{ orgUnitId: number }> };
    expect(sent.targets.map((t) => t.orgUnitId)).toEqual([10, 11]);
    expect(await screen.findByText('Script generation complete')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'View Scripts Folder' })).toBeInTheDocument();
  });

  it('filters the list and selects only what is shown', async () => {
    mockBackend({ test_connection: () => OK_CONNECTION, list_installer_targets: () => targets });
    const user = userEvent.setup();
    render(<App />);
    await openConfigure(user);

    await user.type(screen.getByRole('textbox', { name: 'Filter customers and sites' }), 'zeta');
    expect(screen.queryByRole('checkbox', { name: /^Acme/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Select all' }));
    expect(screen.getByRole('checkbox', { name: 'Zeta Co, customer, ID 20' })).toBeChecked();

    await user.clear(screen.getByRole('textbox', { name: 'Filter customers and sites' }));
    expect(screen.getByRole('checkbox', { name: 'Acme, customer, ID 10' })).not.toBeChecked();
    expect(screen.getByText(/1 of 5 selected/)).toBeInTheDocument();
  });

  it('nests each customer\'s sites under it, and keeps the customer as the heading of a matching site', async () => {
    mockBackend({ test_connection: () => OK_CONNECTION, list_installer_targets: () => targets });
    const user = userEvent.setup();
    const { container } = render(<App />);
    await openConfigure(user);

    const nested = (customerId: string) =>
      [...container.querySelectorAll(`.installer-group:has(input[aria-label$="customer, ID ${customerId}"]) .installer-sites input`)]
        .map((box) => box.getAttribute('aria-label'));
    expect(nested('10')).toEqual(['Acme - Acme HQ, site, ID 11', 'Acme - Acme Depot, site, ID 12']);
    expect(nested('20')).toEqual([]);
    expect(screen.getByText('Sites without a listed customer')).toBeInTheDocument();
    expect(screen.getByRole('checkbox', { name: 'Stray Site, site, ID 99' })).toBeInTheDocument();

    // A site matching the filter keeps its customer above it, and hides the other sites.
    await user.type(screen.getByRole('textbox', { name: 'Filter customers and sites' }), 'depot');
    expect(screen.getByRole('checkbox', { name: 'Acme, customer, ID 10' })).toBeInTheDocument();
    expect(screen.getByRole('checkbox', { name: 'Acme - Acme Depot, site, ID 12' })).toBeInTheDocument();
    expect(screen.queryByRole('checkbox', { name: 'Acme - Acme HQ, site, ID 11' })).not.toBeInTheDocument();
    expect(screen.queryByRole('checkbox', { name: /Zeta/ })).not.toBeInTheDocument();
  });

  it('writes nothing when the sensitive-output warning is declined', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(false);
    const backend = mockBackend({
      test_connection: () => OK_CONNECTION,
      list_installer_targets: () => targets,
    });
    const user = userEvent.setup();
    render(<App />);
    await openConfigure(user);

    await user.click(screen.getByRole('checkbox', { name: 'Zeta Co, customer, ID 20' }));
    await user.click(screen.getByRole('button', { name: /^Generate scripts/ }));

    expect(window.confirm).toHaveBeenCalledOnce();
    expect(backend.callsTo('generate_installer_scripts')).toHaveLength(0);
  });
});