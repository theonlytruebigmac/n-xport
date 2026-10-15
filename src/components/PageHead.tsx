type Step = 'setup' | 'configure' | 'run';

interface PageHeadProps {
    title: string;
    subtitle?: string;
    step: Step;
    /** Label of the last step: Export, Import, Migrate or Generate. */
    runLabel: string;
    onSetup: () => void;
    /** Go back to Configure. Offered once the run has started. */
    onConfigure: () => void;
}

/**
 * The top of every workflow page: what this step is for on the left, and where it sits in the
 * Setup, Configure, Run sequence on the right.
 */
export function PageHead({ title, subtitle, step, runLabel, onSetup, onConfigure }: PageHeadProps) {
    return (
        <div className="page-head">
            <div>
                <h1 className="page-title">{title}</h1>
                {subtitle && <p className="page-subtitle">{subtitle}</p>}
            </div>
            <div className="step-pips" title="Click a completed step to go back to it">
                <button className={`pip ${step === 'setup' ? 'current' : 'done'}`} onClick={onSetup}>
                    <span className="dot" />Setup
                </button>
                <button
                    className={`pip ${step === 'configure' ? 'current' : step === 'run' ? 'done' : 'future'}`}
                    onClick={() => { if (step === 'run') onConfigure(); }}
                >
                    <span className="dot" />Configure
                </button>
                <button className={`pip ${step === 'run' ? 'current' : 'future'}`}>
                    <span className="dot" />{runLabel}
                </button>
            </div>
        </div>
    );
}
