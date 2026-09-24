import { useEffect, useId, useRef, useState } from "react";

const installers = [
  {
    name: "curl",
    command: "curl -fsSL https://ulo.sh/install.sh | sh",
    target: "ulo.sh/install.sh",
  },
  {
    name: "npm",
    command: "npm install -g @arocomputer/ulo",
    target: "@arocomputer/ulo",
  },
  {
    name: "bun",
    command: "bun add -g @arocomputer/ulo",
    target: "@arocomputer/ulo",
  },
  {
    name: "brew",
    command: "brew install arocomputer/tap/ulo",
    target: "arocomputer/tap/ulo",
  },
];

/** Choose and copy an installation command in the hero. */
export function Installer() {
  const id = useId();
  const [method, setMethod] = useState(0);
  const { command: install, target } = installers[method];
  const [prefix, suffix] = install.split(target);
  const [copyState, setCopyState] = useState("");
  const copyReset = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  );

  useEffect(() => () => clearTimeout(copyReset.current), []);

  /** Show success for three seconds after copying; each successful copy restarts the timer. */
  async function copyInstall() {
    try {
      await navigator.clipboard.writeText(install);
      clearTimeout(copyReset.current);
      setCopyState("Copied");
      copyReset.current = setTimeout(() => setCopyState(""), 3000);
    } catch {
      clearTimeout(copyReset.current);
      setCopyState("Select the command to copy it");
    }
  }

  return (
    <>
      <div className="ulo-installer">
        <div
          className="ulo-install-heading"
          role="tablist"
          aria-label="Installation method"
          onKeyDown={(event) => {
            const keys = ["ArrowLeft", "ArrowRight", "Home", "End"];
            if (!keys.includes(event.key)) return;
            event.preventDefault();
            const next =
              event.key === "Home"
                ? 0
                : event.key === "End"
                  ? installers.length - 1
                  : (method +
                      (event.key === "ArrowRight" ? 1 : -1) +
                      installers.length) %
                    installers.length;
            const buttons = event.currentTarget.querySelectorAll("button");
            buttons[next].focus();
            setMethod(next);
            setCopyState("");
          }}
        >
          {installers.map(({ name }, index) => (
            <button
              key={name}
              id={`${id}-tab-${name}`}
              type="button"
              role="tab"
              aria-selected={method === index}
              aria-controls={`${id}-command`}
              tabIndex={method === index ? 0 : -1}
              onClick={() => {
                setMethod(index);
                setCopyState("");
              }}
            >
              {name}
            </button>
          ))}
        </div>
        <div
          className="ulo-install-command"
          id={`${id}-command`}
          role="tabpanel"
          aria-labelledby={`${id}-tab-${installers[method].name}`}
        >
          <button
            className="ulo-copy"
            type="button"
            onClick={copyInstall}
            aria-label="Copy install command"
            data-copied={copyState === "Copied"}
          >
            <code>
              {prefix}
              <strong>{target}</strong>
              {suffix}
            </code>
            <svg viewBox="0 0 24 24" aria-hidden="true">
              {copyState === "Copied" ? (
                <path d="m5 12 4 4L19 6" />
              ) : (
                <>
                  <rect x="8" y="8" width="12" height="12" rx="2" />
                  <path d="M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3" />
                </>
              )}
            </svg>
          </button>
          {copyState && copyState !== "Copied" && (
            <span className="ulo-copy-error">{copyState}</span>
          )}
        </div>
      </div>
      <span className="ulo-visually-hidden" role="status">
        {copyState}
      </span>
    </>
  );
}
