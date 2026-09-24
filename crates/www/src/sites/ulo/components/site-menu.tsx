import { useState } from "react";
import { MenuIcon } from "./menu-icon";

/** Mobile navigation for product pages outside the documentation workspace. */
export function SiteMenu() {
  const [open, setOpen] = useState(false);
  const close = () => setOpen(false);

  return (
    <div className="ulo-site-mobile-menu">
      <button
        className="ulo-site-menu-trigger"
        type="button"
        aria-label={open ? "Close navigation" : "Open navigation"}
        aria-expanded={open}
        aria-controls="mobile-site-navigation"
        onClick={() => setOpen((current) => !current)}
      >
        <MenuIcon open={open} />
      </button>
      <nav
        id="mobile-site-navigation"
        className="ulo-site-mobile-panel"
        aria-label="Main"
        hidden={!open}
      >
        <a href="/ulo" onClick={close}>
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
        <a href="/ulo/docs" onClick={close}>
          Docs
        </a>
        <a href="/ulo/changelog" onClick={close}>
          Changelog
        </a>
      </nav>
    </div>
  );
}
