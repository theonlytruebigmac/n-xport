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

    // Defaults are customers, sites, users. Drop Sites, add User Roles, and add JSON.
    await user.click(await screen.findByRole('checkbox', { name: 'Sites' }));
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
    await user.click(await screen.findByRole('button', { name: /^Start export/ }));

    expect(await screen.findByText('Partial export')).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('Failed to fetch Users for OU 12: timeout');
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
