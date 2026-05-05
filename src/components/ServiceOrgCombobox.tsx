import { useEffect, useRef, useState } from 'react';
import * as api from '../api';
import type { ServiceOrg } from '../types';

interface Props {
    /** Current value as a string (numeric ID, or empty). Kept as string so manual typing is unconstrained. */
    value: string;
    onChange: (value: string) => void;
    /** Whether a connection is established. When false, the combobox is disabled with an "connect first" hint. */
    enabled: boolean;
    placeholder?: string;
    /** Optional pre-selected name to display when the value matches a known SO. If absent the component looks it up. */
    initialName?: string;
}

type FetchState =
    | { kind: 'idle' }
    | { kind: 'loading' }
    | { kind: 'ready'; orgs: ServiceOrg[] }
    | { kind: 'error'; message: string };

/**
 * Typeahead combobox for picking a Service Org after connection. Free-text
 * numeric input is always accepted as an "unverified" fallback so power users
 * can paste an ID without waiting on the discovery list.
 */
export function ServiceOrgCombobox({ value, onChange, enabled, placeholder, initialName }: Props) {
    const [fetchState, setFetchState] = useState<FetchState>({ kind: 'idle' });
    const [open, setOpen] = useState(false);
    const [query, setQuery] = useState(initialName ?? '');
    const wrapperRef = useRef<HTMLDivElement>(null);

    // Auto-fetch on first enable. Going disabled→enabled (e.g., reconnect after
    // disconnect/connecting transition) resets to idle, which re-triggers the load.
    useEffect(() => {
        if (!enabled) {
            setFetchState({ kind: 'idle' });
            return;
        }
        if (fetchState.kind === 'idle') {
            void load();
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [enabled]);

    // Sync local query when the external value/initialName changes
    // (e.g. user switches profiles, or list resolves the name post-fetch).
    useEffect(() => {
        if (initialName) setQuery(initialName);
        else if (!value) setQuery('');
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [value, initialName]);

    // Click-outside closes the popup
    useEffect(() => {
        if (!open) return;
        const onDocClick = (e: MouseEvent) => {
            if (wrapperRef.current && !wrapperRef.current.contains(e.target as Node)) {
                setOpen(false);
            }
        };
        document.addEventListener('mousedown', onDocClick);
        return () => document.removeEventListener('mousedown', onDocClick);
    }, [open]);

    const load = async () => {
        setFetchState({ kind: 'loading' });
        try {
            const orgs = await api.listServiceOrgs();
            setFetchState({ kind: 'ready', orgs });
        } catch (e) {
            setFetchState({ kind: 'error', message: String(e) });
        }
    };

    const orgs = fetchState.kind === 'ready' ? fetchState.orgs : [];

    // Resolve the selected SO's name from the loaded list when available
    const numericValue = value && /^\d+$/.test(value) ? parseInt(value, 10) : null;
    const matchedOrg = numericValue != null ? orgs.find(o => o.id === numericValue) : null;
    const displayName = matchedOrg?.name ?? initialName ?? '';

    // Filter orgs by typed query (matches name OR id substring)
    const filtered = (() => {
        if (!query) return orgs;
        const q = query.toLowerCase();
        return orgs.filter(o =>
            o.name.toLowerCase().includes(q) || o.id.toString().includes(q)
        );
    })();

    const handlePick = (org: ServiceOrg) => {
        onChange(org.id.toString());
        setQuery(org.name);
        setOpen(false);
    };

    const handleType = (val: string) => {
        setQuery(val);
        setOpen(true);
        // If the user typed a pure number, set it directly as the value (manual override path)
        if (/^\d+$/.test(val.trim())) {
            onChange(val.trim());
        } else if (val === '') {
            onChange('');
        }
    };

    const inputValue = open ? query : (displayName || query || '');

    return (
        <div className="combo" ref={wrapperRef}>
            <div className="combo-field" data-disabled={!enabled || undefined}>
                <input
                    className="combo-input"
                    type="text"
                    value={inputValue}
                    placeholder={
                        !enabled
                            ? 'Connect first to load service orgs…'
                            : fetchState.kind === 'loading'
                                ? 'Discovering service orgs…'
                                : (placeholder ?? 'Select or type a service org…')
                    }
                    disabled={!enabled}
                    onFocus={() => { if (enabled) setOpen(true); }}
                    onChange={e => handleType(e.target.value)}
                />
                {value && (
                    <span className="combo-pill" title={matchedOrg ? `Discovered: ${matchedOrg.name}` : 'Manual ID — verify on server'}>
                        {value}
                    </span>
                )}
                {enabled && (
                    <button
                        type="button"
                        className="combo-icon-btn"
                        title="Refresh list"
                        onClick={(e) => { e.stopPropagation(); void load(); }}
                    >
                        {fetchState.kind === 'loading' ? <span className="combo-spinner" /> : '↻'}
                    </button>
                )}
            </div>

            {open && enabled && (
                <div className="combo-pop">
                    <div className="combo-pop-head">
                        <span>
                            {fetchState.kind === 'ready' && `${orgs.length} service org${orgs.length === 1 ? '' : 's'} discovered`}
                            {fetchState.kind === 'loading' && 'Loading…'}
                            {fetchState.kind === 'error' && 'Couldn\'t load list'}
                        </span>
                        <span className="combo-pop-hint">Type to filter…</span>
                    </div>

                    {fetchState.kind === 'error' && (
                        <div className="combo-status-row error">
                            <span>⚠ {fetchState.message}</span>
                        </div>
                    )}

                    {filtered.map(org => {
                        const active = numericValue === org.id;
                        return (
                            <div
                                key={org.id}
                                className={`combo-row ${active ? 'active' : ''}`}
                                onClick={() => handlePick(org)}
                            >
                                <span className="combo-row-name">{org.name}</span>
                                <span className="combo-row-id">{org.id}</span>
                            </div>
                        );
                    })}

                    {fetchState.kind === 'ready' && filtered.length === 0 && (
                        <div className="combo-status-row">No matches for "{query}"</div>
                    )}

                    <div className="combo-pop-foot">Or type a numeric ID directly.</div>
                </div>
            )}

            {!enabled && (
                <div className="combo-status-row" style={{ marginTop: 6, fontSize: 11 }}>
                    Available after the source server connects.
                </div>
            )}

            {numericValue != null && fetchState.kind === 'ready' && !matchedOrg && (
                <div className="combo-status-row" style={{ marginTop: 6, fontSize: 11 }}>
                    ↳ Custom ID — will be used as-is. Verify it exists on the server.
                </div>
            )}
        </div>
    );
}
