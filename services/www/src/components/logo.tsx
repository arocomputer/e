type LogoProps = { height?: number };

/** The ulo wordmark, with the supplied geometry and the surrounding ink color. */
export function Logo({ height = 30 }: LogoProps) {
  return (
    <svg
      width={height * 5}
      height={height}
      viewBox="0 0 150 30"
      fill="currentColor"
      aria-hidden="true"
      focusable="false"
    >
      <path d="M10 30H0V0H10V30Z" />
      <path d="M20 30V0H30V20H50V0H60V30H20Z" />
      <path d="M65 30V0H75V20H105V30H65Z" />
      <path d="M150 30V0H110V30H150ZM140 20H120V10H140V20Z" />
    </svg>
  );
}
