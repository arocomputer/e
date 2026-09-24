import { useState } from "react";
import { MenuIcon } from "../menu-icon";
import type { DocNavGroup } from "./data";

/** Mobile docs navigation opened by the hamburger in the sticky header; `current` is the page's route. */
export function MobileDocsMenu({
  groups,
  current,
}: {
  groups: DocNavGroup[];
  current: string;
}) {
  const [open, setOpen] = useState(false);
  const close = () => setOpen(false);

  return (
    <div className="ulo-doc-mobile-menu">
      <button
        className="ulo-doc-menu-trigger"
        type="button"
        aria-label={open ? "Close navigation" : "Open navigation"}
        aria-expanded={open}
        aria-controls="mobile-doc-navigation"
        onClick={() => setOpen((value) => !value)}
      >
        <MenuIcon open={open} />
      </button>
      <nav
        id="mobile-doc-navigation"
        className="ulo-doc-mobile-panel"
        aria-label="Documentation"
        hidden={!open}
      >
        <div className="ulo-doc-mobile-primary">
          <a href="/" onClick={close}>
            Home
          </a>
          <a
            href="https://github.com/arocomputer/ulo"
            target="_blank"
            rel="noopener noreferrer"
            onClick={close}
          >
            GitHub
          </a>
          <a href="/changelog" onClick={close}>
            Changelog
          </a>
        </div>
        {groups.map((group) => (
          <div className="ulo-doc-group" key={group.label}>
            <p>
              <a
                className="ulo-doc-group-link"
                href={`${group.href}#top`}
                onClick={close}
                aria-current={current === group.href ? "page" : undefined}
              >
                {group.label}
              </a>
            </p>
            {group.links.map(([href, title]) => (
              <a
                key={href}
                href={`${href}#top`}
                onClick={close}
                aria-current={current === href ? "page" : undefined}
              >
                {title}
              </a>
            ))}
          </div>
        ))}
      </nav>
    </div>
  );
}
