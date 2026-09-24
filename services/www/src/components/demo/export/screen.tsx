import type { CSSProperties } from "react";

type Cell = {
  text: string;
  fg: string;
  bg: string;
  reverse: boolean;
  bold: boolean;
  italic: boolean;
};
type ScreenProps = {
  frame: {
    rows: Cell[][];
    cursor: { x: number; y: number; hidden: boolean };
  };
  zoom: number;
  pan: number;
};

/** Preserve captured terminal colors; recordings contain cells, never executable terminal escapes. */
function foreground(color: string) {
  if (/^[0-9a-f]{6}$/i.test(color)) return `#${color}`;
  const ansi: Record<string, string> = {
    default: "#deded8",
    black: "#000",
    red: "#cd0000",
    green: "#00cd00",
    brown: "#cdcd00",
    blue: "#0000ee",
    magenta: "#cd00cd",
    cyan: "#00cdcd",
    white: "#e5e5e5",
    brightblack: "#7f7f7f",
  };
  return ansi[color] || "#deded8";
}

/** Apply recorded terminal attributes, including the reversed cursor cell. */
function cellStyle(cell: Cell): CSSProperties {
  const foregroundColor = foreground(cell.fg);
  const backgroundColor = cell.bg === "default" ? "#000" : foreground(cell.bg);
  return {
    color: cell.reverse ? backgroundColor : foregroundColor,
    backgroundColor: cell.reverse
      ? foregroundColor
      : cell.bg === "default"
        ? "transparent"
        : backgroundColor,
    fontWeight: cell.bold ? 700 : 400,
    fontStyle: cell.italic ? "italic" : "normal",
  };
}

/** Draw cell-sized rails, branches, and arrows without changing transcript text. */
function terminalText(text: string, firstText: string) {
  return text.split(/(●|┃|├|└|↑|↓)/).map((part, index) => {
    if (part === "●" && firstText.startsWith("●"))
      return <span className="ulo-demo-tool-dot" key={index} />;
    if (part === "┃") return <span className="ulo-demo-rail" key={index} />;
    if ((part === "├" || part === "└") && /^[├└]/.test(firstText)) {
      return <span className="ulo-demo-tree" data-branch={part} key={index} />;
    }
    if (part === "↑" || part === "↓") {
      return (
        <svg
          key={index}
          className="ulo-demo-token-arrow"
          viewBox="0 0 10 20"
          aria-hidden="true"
        >
          <path
            d={part === "↑" ? "M5 17V3M2 6l3-3 3 3" : "M5 3v14M2 14l3 3 3-3"}
          />
        </svg>
      );
    }
    return part;
  });
}

/** Render one captured frame in a clipped camera, with no playback state. */
export function TerminalScreen({ frame, zoom, pan }: ScreenProps) {
  return (
    <div
      className="ulo-demo-screen"
      tabIndex={0}
      role="region"
      aria-label="Recorded terminal. ulo explains a slug formatter bug, edits the JavaScript implementation, and runs four passing tests."
    >
      <pre
        aria-hidden="true"
        style={{
          transform: `scale(${zoom}) translateX(-${pan}ch)`,
        }}
      >
        {frame.rows.map((row, y) => (
          <span className="ulo-demo-row" key={y}>
            {row.map((cell, x) => (
              <span key={x} style={cellStyle(cell)}>
                {terminalText(cell.text, row[0].text)}
              </span>
            ))}
          </span>
        ))}
        {!frame.cursor.hidden && (
          <span
            className="ulo-demo-cursor"
            style={{
              left: `${frame.cursor.x}ch`,
              top: `${frame.cursor.y * 1.4}em`,
            }}
          />
        )}
      </pre>
    </div>
  );
}
