import { useCallback, useEffect, useState } from 'react';
import * as api from '../api';
import type { CredentialStorageStatus, LogEntry } from '../types';
import { MIN_PASSPHRASE_LENGTH } from '../types';

interface CredentialStorageCardProps {
    addLog: (level: LogEntry['level'], message: string) => void;
    /** Called after the storage state changes, so saved credentials can be reloaded. */
    onChanged?: () => void | Promise<void>;
}

/**
 * Where saved credentials live. They go in the system keychain. If there is none, nothing is
 * saved to disk unless the user opts in here to an encrypted file protected by a passphrase.
 */
export function CredentialStorageCard({ addLog, onChanged }: CredentialStorageCardProps) {
    const [status, setStatus] = useState<CredentialStorageStatus | null>(null);
    const [editing, setEditing] = useState(false);
    const [passphrase, setPassphrase] = useState('');
    const [confirm, setConfirm] = useState('');
    const [error, setError] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);

    const refresh = useCallback(async () => {
        try {
            setStatus(await api.getCredentialStorageStatus());
        } catch {
            setStatus(null);
        }
    }, []);

    useEffect(() => { void refresh(); }, [refresh]);

    if (!status) return null;

    const isNewFile = !status.vaultExists;
    const reset = () => { setEditing(false); setPassphrase(''); setConfirm(''); setError(null); };

    /** Run a backend call, refresh the status, and report failure inline. */
    const run = async (action: () => Promise<string | void>) => {
        setBusy(true);
        setError(null);
        try {
            const message = await action();
            if (message) addLog('info', message);
            reset();
            await refresh();
            await onChanged?.();
        } catch (e) {
            setError(String(e));
        } finally {
            setBusy(false);
        }
    };

    const enable = () => {
        if (isNewFile && passphrase.length < MIN_PASSPHRASE_LENGTH) {
            setError(`The passphrase must be at least ${MIN_PASSPHRASE_LENGTH} characters.`);
            return;
        }
        if (isNewFile && passphrase !== confirm) {
            setError('The passphrases do not match.');
            return;
        }
        void run(async () => {
            const moved = await api.enableDiskCredentials(passphrase);
            const parts = [`${moved.toKeychain} to the keychain`, `${moved.toEncryptedFile} to the encrypted file`];
            return moved.toKeychain + moved.toEncryptedFile > 0
                ? `Encrypted file storage is on. Moved credentials out of the old file: ${parts.join(', ')}.`
                : 'Encrypted file storage is on.';
        });
    };

    const unlock = () => run(async () => {
        await api.unlockCredentials(passphrase);
        return 'Saved credentials unlocked for this session.';
    });

    const turnOff = (deleteFile: boolean) => {
        if (deleteFile && !confirm_('Delete the encrypted credentials file? Credentials stored in it will be lost and must be entered again.')) return;
        void run(async () => {
            await api.disableDiskCredentials(deleteFile);
            return deleteFile ? 'Encrypted file storage is off and the file was deleted.' : 'Encrypted file storage is off.';
        });
    };

    const passphraseFields = (needsConfirm: boolean, label: string) => (
        <div className="credstore-form">
            <label className="form-label">{label}</label>
            <input
                type="password"
                className="form-input"
                autoComplete="off"
                aria-label={label}
                value={passphrase}
                onChange={e => setPassphrase(e.target.value)}
            />
            {needsConfirm && (
                <>
                    <label className="form-label">Repeat the passphrase</label>
                    <input
                        type="password"
                        className="form-input"
                        autoComplete="off"
                        aria-label="Repeat the passphrase"
                        value={confirm}
                        onChange={e => setConfirm(e.target.value)}
                    />
                    <div className="form-help">
                        At least {MIN_PASSPHRASE_LENGTH} characters. There is no recovery: if you forget it, the
                        saved credentials are lost and must be entered again.
                    </div>
                </>
            )}
        </div>
    );

    let body;
    if (!status.diskEnabled) {
        body = (
            <>
                <p className="credstore-text">
                    Credentials are saved in your system keychain. If there isn't one, they are kept for the
                    current session only and nothing is written to disk.
                </p>
                {status.vaultExists && (
                    <p className="credstore-text">An encrypted credentials file exists but is unreadable while this is off.</p>
                )}
                {editing ? (
                    <>
                        {passphraseFields(isNewFile, isNewFile ? 'New passphrase' : 'Passphrase for the existing file')}
                        <div className="credstore-actions">
                            <button className="btn btn-primary" disabled={busy || !passphrase} onClick={enable}>Turn on</button>
                            <button className="btn btn-secondary" disabled={busy} onClick={reset}>Cancel</button>
                        </div>
                    </>
                ) : (
                    <div className="credstore-actions">
                        <button className="btn btn-secondary" onClick={() => setEditing(true)}>
                            Turn on encrypted file storage…
                        </button>
                        {status.vaultExists && (
                            <button className="btn btn-ghost" disabled={busy} onClick={() => turnOff(true)}>Delete the file</button>
                        )}
                    </div>
                )}
            </>
        );
    } else if (!status.unlocked) {
        body = (
            <>
                <p className="credstore-text">
                    <strong>Locked.</strong> Encrypted file storage is on; enter the passphrase to use saved credentials
                    that are kept in the file.
                </p>
                {status.vaultExists ? passphraseFields(false, 'Passphrase') : passphraseFields(true, 'New passphrase')}
                <div className="credstore-actions">
                    <button className="btn btn-primary" disabled={busy || !passphrase}
                        onClick={status.vaultExists ? unlock : enable}>
                        {status.vaultExists ? 'Unlock' : 'Turn on'}
                    </button>
                    <button className="btn btn-ghost" disabled={busy} onClick={() => turnOff(false)}>Turn off</button>
                    {status.vaultExists && (
                        <button className="btn btn-ghost" disabled={busy} onClick={() => turnOff(true)}>Forgot it? Delete the file</button>
                    )}
                </div>
            </>
        );
    } else {
        body = (
            <>
                <p className="credstore-text">
                    <strong>Unlocked for this session.</strong> If the keychain is unavailable, credentials are saved to
                    an encrypted file protected by your passphrase.
                </p>
                <div className="credstore-actions">
                    <button className="btn btn-secondary" disabled={busy}
                        onClick={() => void run(async () => { await api.lockCredentials(); return 'Saved credentials locked.'; })}>
                        Lock now
                    </button>
                    <button className="btn btn-ghost" disabled={busy} onClick={() => turnOff(false)}>Turn off</button>
                    <button className="btn btn-ghost" disabled={busy} onClick={() => turnOff(true)}>Turn off and delete the file</button>
                </div>
            </>
        );
    }

    return (
        <div className="card card-compact credstore" aria-label="Credential storage">
            <div className="card-header">
                <h2 className="card-title">Credential storage</h2>
            </div>
            {status.legacyEntries > 0 && (
                <div className="soap-api-notice" role="alert">
                    <strong>{status.legacyEntries} saved credential{status.legacyEntries === 1 ? ' is' : 's are'} in an old file that only
                    lightly obfuscates them.</strong>{' '}
                    They move into your keychain automatically when it works. If you have no keychain, turn on
                    encrypted file storage below to protect them.
                </div>
            )}
            {body}
            {error && <div className="credstore-error" role="alert">{error}</div>}
        </div>
    );
}

/** `window.confirm`, wrapped so tests can replace it. */
function confirm_(message: string): boolean {
    return window.confirm(message);
}
