import type { LogEntry } from '../types';
import { CredentialStorageCard } from './CredentialStorageCard';

interface SettingsPanelProps {
    addLog: (level: LogEntry['level'], message: string) => void;
    /** Called after credential storage changes, so saved profiles can be reloaded. */
    onCredentialsChanged: () => void | Promise<void>;
    onBack: () => void;
}

export function SettingsPanel({ addLog, onCredentialsChanged, onBack }: SettingsPanelProps) {
    return (
        <div className="settings-page fade-in">
            <div className="settings-header">
                <button className="btn btn-ghost" onClick={onBack} title="Back to Home">
                    ← Home
                </button>
                <h1>Settings</h1>
            </div>
            <CredentialStorageCard addLog={addLog} onChanged={onCredentialsChanged} />
        </div>
    );
}
