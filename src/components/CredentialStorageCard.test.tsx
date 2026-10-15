import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { CredentialStorageCard } from './CredentialStorageCard';
import { mockBackend } from '../test/backend';
import type { CredentialStorageStatus } from '../types';

const OFF: CredentialStorageStatus = { diskEnabled: false, vaultExists: false, unlocked: false, legacyEntries: 0 };

function renderCard(status: CredentialStorageStatus, handlers: Parameters<typeof mockBackend>[0] = {}) {
  const addLog = vi.fn();
  const onChanged = vi.fn();
  // The status changes as the user acts, as the real backend's would.
  let current = status;
  const backend = mockBackend({
    get_credential_storage_status: () => current,
    enable_disk_credentials: () => {
      current = { ...current, diskEnabled: true, vaultExists: true, unlocked: true };
      return { toKeychain: 0, toEncryptedFile: 0, remaining: 0 };
    },
    unlock_credentials: () => { current = { ...current, unlocked: true }; },
    lock_credentials: () => { current = { ...current, unlocked: false }; },
    disable_disk_credentials: () => { current = { ...current, diskEnabled: false, unlocked: false }; },
    ...handlers,
  });
  render(<CredentialStorageCard addLog={addLog} onChanged={onChanged} />);
  return { backend, addLog, onChanged };
}

afterEach(() => vi.restoreAllMocks());

describe('CredentialStorageCard', () => {
  it('says nothing is written to disk by default and offers the opt-in', async () => {
    renderCard(OFF);
    expect(await screen.findByText(/nothing is written to disk/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Turn on encrypted file storage/ })).toBeInTheDocument();
  });

  it('refuses a short or mismatched passphrase without asking the backend', async () => {
    const user = userEvent.setup();
    const { backend } = renderCard(OFF);
    await user.click(await screen.findByRole('button', { name: /Turn on encrypted file storage/ }));

    await user.type(screen.getByLabelText('New passphrase'), 'too short');
    await user.type(screen.getByLabelText('Repeat the passphrase'), 'too short');
    await user.click(screen.getByRole('button', { name: 'Turn on' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('at least 12 characters');

    await user.clear(screen.getByLabelText('New passphrase'));
    await user.type(screen.getByLabelText('New passphrase'), 'a long enough passphrase');
    await user.click(screen.getByRole('button', { name: 'Turn on' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('do not match');
    expect(backend.callsTo('enable_disk_credentials')).toHaveLength(0);
  });

  it('turns encrypted storage on with the passphrase, then shows it unlocked', async () => {
    const user = userEvent.setup();
    const { backend, onChanged } = renderCard(OFF);
    await user.click(await screen.findByRole('button', { name: /Turn on encrypted file storage/ }));
    await user.type(screen.getByLabelText('New passphrase'), 'a long enough passphrase');
    await user.type(screen.getByLabelText('Repeat the passphrase'), 'a long enough passphrase');
    await user.click(screen.getByRole('button', { name: 'Turn on' }));

    await waitFor(() => expect(backend.callsTo('enable_disk_credentials')).toHaveLength(1));
    expect(backend.callsTo('enable_disk_credentials')[0].args).toEqual({ passphrase: 'a long enough passphrase' });
    expect(await screen.findByText(/Unlocked for this session/)).toBeInTheDocument();
    expect(onChanged).toHaveBeenCalled();
  });

  it('shows a rejected passphrase from the backend and stays locked', async () => {
    const user = userEvent.setup();
    renderCard(
      { diskEnabled: true, vaultExists: true, unlocked: false, legacyEntries: 0 },
      { unlock_credentials: () => { throw new Error('wrong passphrase for the credentials file'); } },
    );
    expect(await screen.findByText(/Locked\./)).toBeInTheDocument();
    await user.type(screen.getByLabelText('Passphrase'), 'not the passphrase');
    await user.click(screen.getByRole('button', { name: 'Unlock' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('wrong passphrase');
    expect(screen.getByText(/Locked\./)).toBeInTheDocument();
  });

  it('unlocks, locks again, and turns off', async () => {
    const user = userEvent.setup();
    const { backend } = renderCard({ diskEnabled: true, vaultExists: true, unlocked: false, legacyEntries: 0 });
    await user.type(await screen.findByLabelText('Passphrase'), 'a long enough passphrase');
    await user.click(screen.getByRole('button', { name: 'Unlock' }));
    expect(await screen.findByText(/Unlocked for this session/)).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Lock now' }));
    expect(await screen.findByText(/Locked\./)).toBeInTheDocument();
    expect(backend.callsTo('lock_credentials')).toHaveLength(1);

    await user.click(screen.getByRole('button', { name: 'Turn off' }));
    expect(await screen.findByText(/nothing is written to disk/)).toBeInTheDocument();
    expect(backend.callsTo('disable_disk_credentials')[0].args).toEqual({ deleteFile: false });
  });

  it('only deletes the encrypted file after confirmation', async () => {
    const user = userEvent.setup();
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false);
    const { backend } = renderCard({ diskEnabled: true, vaultExists: true, unlocked: true, legacyEntries: 0 });

    await user.click(await screen.findByRole('button', { name: 'Turn off and delete the file' }));
    expect(confirm).toHaveBeenCalled();
    expect(backend.callsTo('disable_disk_credentials')).toHaveLength(0);

    confirm.mockReturnValue(true);
    await user.click(screen.getByRole('button', { name: 'Turn off and delete the file' }));
    await waitFor(() => expect(backend.callsTo('disable_disk_credentials')).toHaveLength(1));
    expect(backend.callsTo('disable_disk_credentials')[0].args).toEqual({ deleteFile: true });
  });

  it('warns about credentials left in the old lightly obfuscated file', async () => {
    renderCard({ ...OFF, legacyEntries: 2 });
    expect(await screen.findByRole('alert')).toHaveTextContent('2 saved credentials are in an old file');
  });
});
