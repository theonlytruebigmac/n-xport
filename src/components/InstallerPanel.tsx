import { useEffect, useMemo, useState } from 'react';
import * as api from '../api';
import type { InstallerTarget, LogEntry } from '../types';
import { installerTargetKey } from '../types';
import { ServiceOrgCombobox } from './ServiceOrgCombobox';

interface InstallerPanelProps {
    serviceOrgId: string;
    setServiceOrgId: (v: string) => void;
    outputDir: string;
    setOutputDir: (v: string) => void;
    targets: InstallerTarget[];
    setTargets: (targets: InstallerTarget[]) => void;
    selected: Set<string>;
    setSelected: (selected: Set<string>) => void;
    onBrowseOutput: () => void;
    onBack: () => void;
    addLog: (level: LogEntry['level'], message: string) => void;
    connectedServiceOrgName?: string;
}

/** Choose the customers and sites to generate Windows agent installer scripts for. */
export function InstallerPanel({
    serviceOrgId, setServiceOrgId,
    outputDir, setOutputDir,
    targets, setTargets,
    selected, setSelected,
    onBrowseOutput,
    onBack,
    addLog,
    connectedServiceOrgName,
}: InstallerPanelProps) {
    const [filter, setFilter] = useState('');
    const [loading, setLoading] = useState(false);

    // Load the customers and sites whenever a valid service org is chosen. Debounced so typing
    // an ID does not fire a request per digit.
    useEffect(() => {
        if (!/^\d+$/.test(serviceOrgId)) {
            setTargets([]);
            return;
        }
        let current = true;
        const handle = setTimeout(async () => {
            setLoading(true);
            try {
                const loaded = await api.listInstallerTargets(parseInt(serviceOrgId, 10));
                if (!current) return;
                setTargets(loaded);
                setSelected(new Set());
                addLog('info', `Found ${loaded.length} customers and sites.`);
            } catch (e) {
                if (!current) return;
                setTargets([]);
                addLog('error', `Failed to list customers and sites: ${e}`);
            } finally {
                if (current) setLoading(false);
            }
        }, 400);
        return () => { current = false; clearTimeout(handle); };
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [serviceOrgId]);

    // Customers with their sites nested under them. Sites with no listed customer go last.
    const groups = useMemo(() => {
        const byCustomer = new Map<number, { customer: InstallerTarget | null; sites: InstallerTarget[] }>();
        const loose: InstallerTarget[] = [];
        for (const target of targets) {
            if (target.orgUnitType === 'customer') {
                byCustomer.set(target.orgUnitId, { customer: target, sites: [] });
            }
        }
        for (const target of targets) {
            if (target.orgUnitType !== 'site') continue;
            const group = target.customerId != null ? byCustomer.get(target.customerId) : undefined;
            if (group) group.sites.push(target);
            else loose.push(target);
        }
        const result = [...byCustomer.values()];
        if (loose.length > 0) result.push({ customer: null, sites: loose });
        return result;
    }, [targets]);

    // The filter keeps a customer as the heading of any site that matches; a matching customer
    // keeps all of its sites.
    const shownGroups = useMemo(() => {
        const needle = filter.trim().toLowerCase();
        const matches = (t: InstallerTarget) =>
            !needle
            || t.name.toLowerCase().includes(needle)
            || (t.customerName ?? '').toLowerCase().includes(needle)
            || String(t.orgUnitId).includes(needle);
        return groups
            .map(group => {
                const customerMatches = group.customer !== null && matches(group.customer);
                const sites = customerMatches ? group.sites : group.sites.filter(matches);
                return { customer: group.customer, sites, show: customerMatches || sites.length > 0 };
            })
            .filter(group => group.show);
    }, [groups, filter]);

    const visible = useMemo(
        () => shownGroups.flatMap(group => [...(group.customer ? [group.customer] : []), ...group.sites]),
        [shownGroups],
    );

    const allVisibleSelected = visible.length > 0 && visible.every(t => selected.has(installerTargetKey(t)));

    const toggle = (target: InstallerTarget) => {
        const next = new Set(selected);
        const key = installerTargetKey(target);
        if (next.has(key)) next.delete(key);
        else next.add(key);
        setSelected(next);
    };

    const toggleAllVisible = () => {
        const next = new Set(selected);
        for (const target of visible) {
            if (allVisibleSelected) next.delete(installerTargetKey(target));
            else next.add(installerTargetKey(target));
        }
        setSelected(next);
    };

    const renderRow = (target: InstallerTarget, nested: boolean) => {
        const key = installerTargetKey(target);
        const fullName = target.orgUnitType === 'site' && target.customerName
            ? `${target.customerName} - ${target.name}`
            : target.name;
        return (
            <label key={key} className={`checkbox-item installer-row ${nested ? 'installer-site' : 'installer-customer'} ${selected.has(key) ? 'selected' : ''}`}>
                <input
                    type="checkbox"
                    aria-label={`${fullName}, ${target.orgUnitType}, ID ${target.orgUnitId}`}
                    checked={selected.has(key)}
                    onChange={() => toggle(target)}
                />
                <span className="installer-name">{target.name}</span>
                <span className="installer-kind">{target.orgUnitType === 'site' ? 'Site' : 'Customer'}</span>
                <span className="installer-id">{target.orgUnitId}</span>
            </label>
        );
    };

    return (
        <div className="card card-compact fade-in">
            <div className="grid-2">
                <div className="form-group">
                    <label className="form-label">Target Service Org</label>
                    <ServiceOrgCombobox
                        value={serviceOrgId}
                        onChange={setServiceOrgId}
                        enabled={true}
                        initialName={connectedServiceOrgName}
                        placeholder="Select or type a service org…"
                    />
                </div>
                <div className="form-group">
                    <label className="form-label">Save Scripts In</label>
                    <div style={{ display: 'flex', gap: 'var(--space-sm)' }}>
                        <input type="text" className="form-input" value={outputDir} onChange={e => setOutputDir(e.target.value)} />
                        <button className="btn btn-secondary" onClick={onBrowseOutput}>Browse</button>
                    </div>
                </div>
            </div>

            <div className="form-group">
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
                    <label className="form-label" style={{ marginBottom: 0 }}>
                        Customers and Sites
                        <span className="installer-count">
                            {loading ? ' · loading…' : ` · ${selected.size} of ${targets.length} selected`}
                        </span>
                    </label>
                    <button
                        type="button"
                        className="btn btn-link"
                        style={{ marginRight: -6 }}
                        disabled={visible.length === 0}
                        onClick={toggleAllVisible}
                    >
                        {allVisibleSelected ? 'Clear all' : 'Select all'}
                    </button>
                </div>
                <input
                    type="text"
                    className="form-input"
                    placeholder="Filter by name or ID…"
                    aria-label="Filter customers and sites"
                    value={filter}
                    onChange={e => setFilter(e.target.value)}
                />
                <div className="installer-list" role="group" aria-label="Customers and sites">
                    {visible.length === 0 && (
                        <div className="installer-empty">
                            {loading ? 'Loading…' : targets.length === 0 ? 'Choose a service org to list its customers and sites.' : 'Nothing matches the filter.'}
                        </div>
                    )}
                    {shownGroups.map(group => (
                        <div key={group.customer ? `customer:${group.customer.orgUnitId}` : 'loose'} className="installer-group">
                            {group.customer ? renderRow(group.customer, false) : (
                                <div className="installer-group-title">Sites without a listed customer</div>
                            )}
                            {group.sites.length > 0 && (
                                <div className="installer-sites">
                                    {group.sites.map(site => renderRow(site, true))}
                                </div>
                            )}
                        </div>
                    ))}
                </div>
                <div className="form-help">Each script contains a registration token in plain text. Store and share the files securely.</div>
            </div>

            <div className="card-actions">
                <span />
                <div style={{ display: 'flex', gap: 8 }}>
                    <button className="btn btn-secondary" onClick={onBack}>Back</button>
                </div>
            </div>
        </div>
    );
}
