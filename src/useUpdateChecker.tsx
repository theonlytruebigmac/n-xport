// Update checker component for React frontend
import { useState, useEffect } from 'react';
import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

interface UpdateInfo {
    available: boolean;
    currentVersion: string;
    newVersion?: string;
    body?: string;
}

export function useUpdateChecker() {
    const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
    const [checking, setChecking] = useState(false);
    const [downloading, setDownloading] = useState(false);
    const [progress, setProgress] = useState(0);

    const checkForUpdate = async () => {
        setChecking(true);
        try {
            const update = await check();

            if (update) {
                setUpdateInfo({
                    available: true,
                    currentVersion: update.currentVersion,
                    newVersion: update.version,
                    body: update.body
                });
            } else {
                setUpdateInfo({
                    available: false,
                    currentVersion: 'unknown'
                });
            }
        } catch (e) {
            console.error('Failed to check for updates:', e);
        } finally {
            setChecking(false);
        }
    };

    const downloadAndInstall = async () => {
        if (!updateInfo?.available) return;

        setDownloading(true);
        try {
            const update = await check();
            if (update) {
                await update.downloadAndInstall((event) => {
                    if (event.event === 'Started' && event.data.contentLength) {
                        setProgress(0);
                    } else if (event.event === 'Progress') {
                        setProgress(prev => prev + (event.data.chunkLength || 0));
                    } else if (event.event === 'Finished') {
                        setProgress(100);
                    }
                });

                // Relaunch the app after update
                await relaunch();
            }
        } catch (e) {
            console.error('Failed to install update:', e);
        } finally {
            setDownloading(false);
        }
    };

    // Check for updates on mount (after a delay)
    useEffect(() => {
        const timeout = setTimeout(() => {
            checkForUpdate();
        }, 5000); // Wait 5 seconds after app loads

        return () => clearTimeout(timeout);
    }, []);

    return {
        updateInfo,
        checking,
        downloading,
        progress,
        checkForUpdate,
        downloadAndInstall
    };
}

// Simple update notification banner component
export function UpdateBanner() {
    const { updateInfo, checking, downloading, downloadAndInstall } = useUpdateChecker();

    if (checking) {
        return null; // Don't show anything while checking
    }

    if (!updateInfo?.available) {
        return null;
    }

    return (
        <div style={{
            background: 'var(--color-info-bg)',
            borderRadius: 'var(--radius-md)',
            padding: 'var(--space-sm) var(--space-md)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            marginBottom: 'var(--space-md)',
            border: '1px solid var(--color-info)'
        }}>
            <span style={{ fontSize: '0.875rem', color: 'var(--color-info)' }}>
                Update available: v{updateInfo.newVersion}
            </span>
            <button
                className="btn btn-secondary"
                style={{ fontSize: '0.75rem', padding: 'var(--space-xs) var(--space-sm)' }}
                onClick={downloadAndInstall}
                disabled={downloading}
            >
                {downloading ? 'Installing...' : 'Update Now'}
            </button>
        </div>
    );
}
