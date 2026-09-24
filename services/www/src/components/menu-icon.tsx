/** Two-line menu mark that becomes a close icon when navigation is open. */
export function MenuIcon({ open }: { open: boolean }) {
  return (
    <svg className="ulo-menu-icon" viewBox="0 0 24 24" aria-hidden="true">
      {open ? (
        <path d="M6.5 6.5l11 11m0-11-11 11" />
      ) : (
        <path d="M5 8.5h14M5 15.5h14" />
      )}
    </svg>
  );
}
