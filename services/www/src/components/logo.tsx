type LogoProps = { height?: number };

/** The ulo wordmark without a leading bar, using the surrounding ink color. */
export function Logo({ height = 30 }: LogoProps) {
  return (
    <svg
      width={(height * 13) / 3}
      height={height}
      viewBox="20 0 130 30"
      fill="currentColor"
      aria-hidden="true"
      focusable="false"
    >
      <path d="M20 30V0H30V20H50V0H60V30H20Z" />
      <path d="M65 30V0H75V20H105V30H65Z" />
      <path d="M150 30V0H110V30H150ZM140 20H120V10H140V20Z" />
    </svg>
  );
}
