/** What must already exist in N-central before custom property values can be imported. */
export function PropertyPrerequisitesNotice({ resource }: { resource: 'org_properties' | 'device_properties' }) {
    return (
        <div className="soap-api-notice" role="note">
            <strong>Create these in N-central first.</strong>{' '}
            N-xport sets property values only: the custom property definition must already exist
            {resource === 'device_properties'
                ? ', and the device must already be on this server (install or register its agent first; N-xport does not create or move devices).'
                : '.'}
            {' '}Rows that refer to a missing definition{resource === 'device_properties' ? ' or device' : ''} are reported as errors.
        </div>
    );
}
