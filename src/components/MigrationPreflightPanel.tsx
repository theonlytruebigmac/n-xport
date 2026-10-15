import { useEffect, useState } from 'react';
import * as api from '../api';
import type { MigrationPreflight, SoapUse } from '../types';
import { toMigrationOptions } from '../types';
import { SoapApiNotice } from './SoapApiNotice';

interface MigrationPreflightPanelProps {
    selectedTypes: Set<string>;
    sourceSoId?: number;
    destSoId?: number;
}

const SOAP_LABELS: Record<SoapUse, string> = {
    none: 'REST only',
    fallback: 'SOAP fallback',
    preferred: 'SOAP preferred',
    required: 'SOAP required',
};

/** Shows what a migration will do before it starts, as described by the backend. */
export function MigrationPreflightPanel({ selectedTypes, sourceSoId, destSoId }: MigrationPreflightPanelProps) {
    const [preflight, setPreflight] = useState<MigrationPreflight | null>(null);
    const [error, setError] = useState<string | null>(null);

    // The Set is replaced on every toggle, so a sorted key is a stable dependency.
    const selectionKey = [...selectedTypes].sort().join(',');

    useEffect(() => {
        let current = true;
        api.migrationPreflight(toMigrationOptions(selectedTypes), sourceSoId ?? 0, destSoId ?? 0)
            .then(result => {
                if (current) {
                    setPreflight(result);
                    setError(null);
                }
            })
            .catch(e => {
                if (current) setError(String(e));
            });
        return () => { current = false; };
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [selectionKey, sourceSoId, destSoId]);

    if (error) {
        return <div className="preflight-panel" role="note">Preflight summary unavailable: {error}</div>;
    }
    if (!preflight) return null;

    return (
        <div className="preflight-panel">
            <div className="preflight-title">Preflight summary</div>
            {sourceSoId !== undefined && destSoId !== undefined && (
                <div className="preflight-scope">Source SO {sourceSoId} → Destination SO {destSoId}</div>
            )}
            {preflight.warnings.map(warning => (
                <div key={warning} className="preflight-warning">{warning}</div>
            ))}
            {preflight.prerequisites.length > 0 && (
                <div className="preflight-prereq" role="note">
                    <strong>Before you start</strong>
                    <ul>
                        {preflight.prerequisites.map(item => <li key={item}>{item}</li>)}
                    </ul>
                </div>
            )}
            {preflight.steps.length > 0 && (
                <ol className="preflight-steps">
                    {preflight.steps.map(step => (
                        <li key={step.resource}>
                            <span className="preflight-step-label">{step.label}</span>
                            <span className={`preflight-soap preflight-soap-${step.soap}`}>{SOAP_LABELS[step.soap]}</span>
                            <div className="preflight-step-details">{step.details}</div>
                        </li>
                    ))}
                </ol>
            )}
            {preflight.manualFollowUp.length > 0 && (
                <div className="preflight-followup">
                    <strong>Known manual follow-up</strong>
                    <ul>
                        {preflight.manualFollowUp.map(item => <li key={item}>{item}</li>)}
                    </ul>
                </div>
            )}
            {preflight.usesSoap && <SoapApiNotice />}
        </div>
    );
}
