import { render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { MigrationPreflightPanel } from './MigrationPreflightPanel';
import { mockBackend } from '../test/backend';
import type { MigrationPreflight } from '../types';

const preflight = (overrides: Partial<MigrationPreflight> = {}): MigrationPreflight => ({
  sourceSoId: 1,
  destSoId: 2,
  steps: [],
  usesSoap: false,
  prerequisites: [],
  warnings: [],
  manualFollowUp: [],
  ...overrides,
});

describe('MigrationPreflightPanel', () => {
  it('asks the backend about the selection, source and destination, and shows the steps in order', async () => {
    const backend = mockBackend({
      migration_preflight: () =>
        preflight({
          usesSoap: true,
          steps: [
            { resource: 'customers', label: 'Customers and sites', soap: 'fallback', details: 'REST first.' },
            { resource: 'users', label: 'Users', soap: 'required', details: 'SOAP only.' },
          ],
          manualFollowUp: ['Users are created with generated passwords.'],
        }),
    });

    render(<MigrationPreflightPanel selectedTypes={new Set(['customers', 'users'])} sourceSoId={1} destSoId={2} />);

    expect(await screen.findByText('Customers and sites')).toBeInTheDocument();
    expect(backend.callsTo('migration_preflight')[0].args).toMatchObject({
      options: { customers: true, users: true, userRoles: false, accessGroups: false },
      sourceSoId: 1,
      destSoId: 2,
    });
    expect(screen.getByText('Source SO 1 → Destination SO 2')).toBeInTheDocument();
    const items = screen.getAllByRole('listitem').map((item) => item.textContent ?? '');
    expect(items[0]).toContain('Customers and sites');
    expect(items[0]).toContain('SOAP fallback');
    expect(items[1]).toContain('Users');
    expect(items[1]).toContain('SOAP required');
    expect(screen.getByText('Users are created with generated passwords.')).toBeInTheDocument();
  });

  it('shows the SOAP notice only when the backend says a step may use SOAP', async () => {
    mockBackend({ migration_preflight: () => preflight({ usesSoap: true }) });
    const { unmount } = render(<MigrationPreflightPanel selectedTypes={new Set(['users'])} />);
    expect(await screen.findByText('SOAP API may be required.')).toBeInTheDocument();
    unmount();

    mockBackend({ migration_preflight: () => preflight({ usesSoap: false }) });
    render(<MigrationPreflightPanel selectedTypes={new Set(['device_properties'])} />);
    expect(await screen.findByText('Preflight summary')).toBeInTheDocument();
    expect(screen.queryByText('SOAP API may be required.')).not.toBeInTheDocument();
  });

  it('asks again when the selection changes and shows selection warnings', async () => {
    const backend = mockBackend({
      migration_preflight: () => preflight({ warnings: ['No data is selected, so nothing will be migrated.'] }),
    });
    const { rerender } = render(<MigrationPreflightPanel selectedTypes={new Set<string>()} />);
    expect(await screen.findByText('No data is selected, so nothing will be migrated.')).toBeInTheDocument();

    rerender(<MigrationPreflightPanel selectedTypes={new Set(['access_groups'])} />);
    await waitFor(() => expect(backend.callsTo('migration_preflight')).toHaveLength(2));
    expect(backend.callsTo('migration_preflight')[1].args).toMatchObject({ options: { accessGroups: true } });
  });

  it('lists what must already exist on the destination before the run, apart from follow-up', async () => {
    mockBackend({
      migration_preflight: () =>
        preflight({
          prerequisites: [
            'Organization custom property definitions must already exist on the destination.',
            'The devices must already be on the destination.',
          ],
          manualFollowUp: ['Users are created with generated passwords.'],
        }),
    });
    render(<MigrationPreflightPanel selectedTypes={new Set(['org_properties', 'device_properties'])} />);

    const before = await screen.findByRole('note', { name: '' });
    expect(before).toHaveTextContent('Before you start');
    expect(before).toHaveTextContent('Organization custom property definitions must already exist');
    expect(before).toHaveTextContent('The devices must already be on the destination.');
    // Follow-up is a separate list and is not mixed into the prerequisites.
    expect(before).not.toHaveTextContent('generated passwords');
    expect(screen.getByText('Known manual follow-up')).toBeInTheDocument();
  });

  it('shows no "Before you start" box when there are no prerequisites', async () => {
    mockBackend({ migration_preflight: () => preflight() });
    render(<MigrationPreflightPanel selectedTypes={new Set(['customers'])} />);
    expect(await screen.findByText('Preflight summary')).toBeInTheDocument();
    expect(screen.queryByText('Before you start')).not.toBeInTheDocument();
  });

  it('says so when the summary cannot be loaded', async () => {
    mockBackend({ migration_preflight: () => { throw new Error('backend down'); } });
    render(<MigrationPreflightPanel selectedTypes={new Set(['users'])} />);
    expect(await screen.findByText(/Preflight summary unavailable/)).toHaveTextContent('backend down');
  });
});
