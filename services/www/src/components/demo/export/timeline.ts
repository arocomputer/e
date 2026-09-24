type CapturedFrame = {
  duration: number;
  rows: { text: string; reverse: boolean }[][];
};

/** Time the live draft at 2× speed and follow its painted cursor during the 2× close-up. */
export function terminalPlayback(frames: CapturedFrame[]) {
  let start = 0;
  return frames.map((frame) => {
    const draft = frame.rows.find((row, index) => {
      const text = row.map((cell) => cell.text).join("");
      const continuation = frame.rows[index - 1]
        ?.map((cell) => cell.text)
        .join("")
        .startsWith("┃ ");
      return (
        text.startsWith("┃ ") &&
        (text.slice(2).trim() !== "" || continuation) &&
        row.some((cell) => cell.reverse)
      );
    });
    const typing = draft !== undefined;
    let cursor = 0;
    for (const cell of draft ?? []) {
      if (cell.reverse) break;
      cursor += [...cell.text].length;
    }
    const duration = frame.duration / (typing ? 2 : 1);
    const result = {
      start,
      duration,
      typing,
      pan: Math.max(0, Math.min(50, cursor - 35)),
    };
    start += duration;
    return result;
  });
}
