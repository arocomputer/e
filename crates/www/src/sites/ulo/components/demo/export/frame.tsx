"use client";

import { useEffect, useState } from "react";
import { flushSync } from "react-dom";
import frames from "./recording.json";
import { terminalPlayback } from "./timeline";
import { TerminalScreen } from "./screen";
import "./screen.css";

const timeline = terminalPlayback(frames);
const duration = timeline.reduce((sum, frame) => sum + frame.duration, 0);

/** Export-only renderer: the capture script advances its clock one frame at a time. */
export default function VideoFrame() {
  const [time, setTime] = useState(0);
  useEffect(() => {
    const target = window as typeof window & {
      renderDemoFrame?: (time: number) => void;
      demoDuration?: number;
    };
    target.renderDemoFrame = (value) => flushSync(() => setTime(value));
    target.demoDuration = duration;
    return () => {
      delete target.renderDemoFrame;
      delete target.demoDuration;
    };
  }, []);
  const index = Math.max(
    0,
    timeline.findLastIndex((frame) => frame.start <= time),
  );
  const shot = timeline[index];
  return (
    <div
      className="ulo-demo"
      style={{ width: 1080, height: 676, overflow: "hidden" }}
    >
      <TerminalScreen
        frame={frames[index]}
        zoom={shot.typing ? 2 : 1}
        pan={shot.pan}
      />
    </div>
  );
}
