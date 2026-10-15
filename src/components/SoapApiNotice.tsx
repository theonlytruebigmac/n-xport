export function SoapApiNotice() {
    return (
        <div className="soap-api-notice" role="note">
            <strong>SOAP API may be required.</strong>{' '}
            Some selected operations use SOAP as a fallback, and user creation requires it.
            SOAP APIs are disabled by default in N-central. Enable them on the server before
            running these operations, or SOAP-dependent rows may fail.
        </div>
    );
}
