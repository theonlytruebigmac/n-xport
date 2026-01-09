// Tauri API wrapper functions
import { invoke } from '@tauri-apps/api/core';
import type {
    Profile,
    Settings,
    ConnectionResult,
    ExportType
} from './types';

// Connection commands
export async function testConnection(fqdn: string, jwt: string): Promise<ConnectionResult> {
    return invoke('test_connection', { fqdn, jwt });
}

export async function connectWithProfile(profileName: string, fqdn: string): Promise<ConnectionResult> {
    return invoke('connect_with_profile', { profileName, fqdn });
}

export async function saveCredentials(profileName: string, jwt: string): Promise<void> {
    return invoke('save_credentials', { profileName, jwt });
}

export async function hasCredentials(profileName: string): Promise<boolean> {
    return invoke('has_credentials', { profileName });
}

export async function getCredentials(profileName: string): Promise<string | null> {
    return invoke('get_credentials', { profileName });
}

export async function deleteCredentials(profileName: string): Promise<void> {
    return invoke('delete_credentials', { profileName });
}

export async function disconnect(): Promise<void> {
    return invoke('disconnect');
}

export async function getServiceOrgInfo(serviceOrgId: number): Promise<{ id: number, name: string }> {
    return invoke('get_service_org_info', { serviceOrgId });
}

// Config commands
export async function getSettings(): Promise<Settings> {
    return invoke('get_settings');
}

export async function saveSettings(settings: Settings): Promise<void> {
    return invoke('save_settings', { settings });
}

export async function getProfiles(): Promise<Profile[]> {
    return invoke('get_profiles');
}

export async function saveProfile(profile: Profile): Promise<void> {
    return invoke('save_profile', { profile });
}

export async function deleteProfile(name: string): Promise<void> {
    return invoke('delete_profile', { name });
}

export async function setActiveProfile(name: string): Promise<void> {
    return invoke('set_active_profile', { name });
}

export async function getActiveProfile(): Promise<Profile | null> {
    return invoke('get_active_profile');
}

// Open directory command
export async function openDirectory(path: string): Promise<void> {
    return invoke('open_directory', { path });
}

// Export commands
export async function startExport(
    outputDir: string,
    options: any,
    formats: string[],
    serviceOrgId: number
): Promise<any> {
    return invoke('start_export', {
        outputDir,
        options,
        formats,
        serviceOrgId
    });
}

export async function startMigration(options: any, sourceSoId: number, destSoId: number): Promise<any> {
    return invoke('start_migration', { options, sourceSoId, destSoId });
}

export async function getExportTypes(): Promise<ExportType[]> {
    return invoke('get_export_types');
}
