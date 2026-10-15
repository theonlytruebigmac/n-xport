import { useEffect, useRef, useState } from 'react';
import { save } from '@tauri-apps/plugin-dialog';
import * as api from '../api';
import type { ImportType, LogEntry } from '../types';

interface ImportTemplatesMenuProps {
    addLog: (level: LogEntry['level'], message: string) => void;
}

/**
 * Download a starting CSV for an import resource. Templates are generated locally, so this
 * works without a server connection and is offered both before connecting and when configuring.
 */
export function ImportTemplatesMenu({ addLog }: ImportTemplatesMenuProps) {
    const [types, setTypes] = useState<ImportType[]>([]);
    const [open, setOpen] = useState(false);
    const menuRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        api.getImportTypes()
            .then(setTypes)
            .catch((e) => addLog('error', `Failed to load import types: ${e}`));
    }, [addLog]);

    useEffect(() => {
        if (!open) return;
        const onDocClick = (e: MouseEvent) => {
            if (menuRef.current && !menuRef.current.contains(e.target as Node)) setOpen(false);
        };
        document.addEventListener('mousedown', onDocClick);
        return () => document.removeEventListener('mousedown', onDocClick);
    }, [open]);

    const download = async (resourceId: string, resourceName: string) => {
        setOpen(false);
        try {
            const path = await save({
                title: `Save ${resourceName} template`,
                defaultPath: `${resourceId}_template.csv`,
                filters: [{ name: 'CSV', extensions: ['csv'] }],
            });
            if (!path) return;
            const written = await api.generateTemplate(resourceId, path);
            addLog('success', `Template saved to ${written}`);
        } catch (e) {
            addLog('error', `Failed to save template: ${e}`);
        }
    };

    return (
        <div className="templates-menu" ref={menuRef}>
            <button className="templates-trigger" onClick={() => setOpen(o => !o)}>
                <span>↓</span> CSV templates <span className="caret">▾</span>
            </button>
            {open && (
                <div className="templates-pop">
                    <div className="templates-pop-head">Download a starting CSV</div>
                    {types.filter(t => t.supported).map(t => (
                        <button key={t.id} className="templates-pop-item" onClick={() => download(t.id, t.name)}>
                            <span>{t.name}</span>
                            <span className="arrow">↓</span>
                        </button>
                    ))}
                </div>
            )}
        </div>
    );
}
