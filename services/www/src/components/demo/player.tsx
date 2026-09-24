import { useEffect, useRef, useState } from "react";

/** Native video playback with the product's controls and offscreen/background pausing. */
export function TerminalDemo() {
  const container = useRef<HTMLElement>(null);
  const video = useRef<HTMLVideoElement>(null);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [wantsToPlay, setWantsToPlay] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [visible, setVisible] = useState(false);
  const [foregroundTab, setForegroundTab] = useState(true);
  const [failed, setFailed] = useState(false);
  const ended = duration > 0 && time >= duration;
  const progress = duration ? Math.min(100, (time / duration) * 100) : 0;

  /** Keep picture clicks, the icon, and keyboard controls on the same media clock. */
  function togglePlayback() {
    if (ended && video.current) {
      video.current.currentTime = 0;
      setTime(0);
    }
    setWantsToPlay(ended || Boolean(video.current?.paused));
  }
  function seek(value: number) {
    if (!video.current) return;
    const next = Math.max(0, Math.min(duration, value));
    video.current.currentTime = next;
    setTime(next);
  }
  useEffect(() => {
    const motion = matchMedia("(prefers-reduced-motion: reduce)");
    const preference = () => setWantsToPlay(!motion.matches);
    preference();
    motion.addEventListener("change", preference);
    const observer = new IntersectionObserver(
      ([entry]) => setVisible(entry.isIntersecting),
      { threshold: 0.2 },
    );
    if (container.current) observer.observe(container.current);
    const visibility = () => setForegroundTab(!document.hidden);
    document.addEventListener("visibilitychange", visibility);
    visibility();
    return () => {
      observer.disconnect();
      motion.removeEventListener("change", preference);
      document.removeEventListener("visibilitychange", visibility);
    };
  }, []);
  useEffect(() => {
    const media = video.current;
    if (!media) return;
    const syncDuration = () => {
      if (Number.isFinite(media.duration) && media.duration > 0)
        setDuration(media.duration);
    };
    syncDuration();
    media.addEventListener("loadedmetadata", syncDuration);
    media.addEventListener("durationchange", syncDuration);
    return () => {
      media.removeEventListener("loadedmetadata", syncDuration);
      media.removeEventListener("durationchange", syncDuration);
    };
  }, []);
  useEffect(() => {
    const media = video.current;
    if (!media) return;
    if (wantsToPlay && visible && foregroundTab) {
      void media.play().catch((error: unknown) => {
        if (!(error instanceof DOMException && error.name === "AbortError"))
          setWantsToPlay(false);
      });
    } else media.pause();
  }, [wantsToPlay, visible, foregroundTab]);
  useEffect(() => {
    if (!playing) return;
    let frame = 0;
    const tick = () => {
      if (video.current) setTime(video.current.currentTime);
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [playing]);
  const action = ended ? "Replay demo" : playing ? "Pause demo" : "Play demo";
  return (
    <figure
      ref={container}
      className="ulo-demo ulo-demo-player"
      aria-label="Recorded ulo coding session"
    >
      <video
        ref={video}
        className="ulo-demo-video"
        src="/demo.mp4"
        poster="/demo-poster.jpg"
        width={2160}
        height={1352}
        muted
        playsInline
        preload="metadata"
        draggable={false}
        role="button"
        tabIndex={0}
        aria-label={`${action} video`}
        onClick={togglePlayback}
        onDragStart={(event) => event.preventDefault()}
        onKeyDown={(event) => {
          if (event.key === " " || event.key === "Enter") {
            event.preventDefault();
            togglePlayback();
          }
        }}
        onTimeUpdate={(event) => setTime(event.currentTarget.currentTime)}
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        onEnded={() => setWantsToPlay(false)}
        onError={() => setFailed(true)}
      />
      {failed && (
        <p className="ulo-demo-error" role="status">
          The demo could not load. <a href="/demo.mp4">Open the video</a>.
        </p>
      )}
      <div className="ulo-demo-controls">
        <button
          type="button"
          className="ulo-demo-toggle"
          aria-label={action}
          onClick={togglePlayback}
        >
          <svg viewBox="0 0 8 12" aria-hidden="true">
            {playing ? (
              <path d="M0 1h3v10H0zm5 0h3v10H5z" />
            ) : (
              <path d="M0 0 8 6l-8 6z" />
            )}
          </svg>
        </button>
        <div className="ulo-demo-timeline">
          <span className="ulo-demo-track" aria-hidden="true">
            <span
              className="ulo-demo-progress"
              style={{ width: `${progress}%` }}
            />
          </span>
          <input
            aria-label="Demo timeline"
            type="range"
            min={0}
            max={duration}
            step={0.01}
            value={time}
            disabled={!duration}
            onPointerDown={(event) => {
              event.currentTarget.setPointerCapture(event.pointerId);
              setWantsToPlay(false);
            }}
            onPointerUp={() => setWantsToPlay(true)}
            onPointerCancel={() => setWantsToPlay(true)}
            onKeyDown={(event) => {
              if (event.key === " " || event.key === "Enter") {
                event.preventDefault();
                togglePlayback();
              } else {
                setWantsToPlay(false);
                if (
                  ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(
                    event.key,
                  )
                ) {
                  event.preventDefault();
                  seek(
                    time +
                      (event.key === "ArrowRight" || event.key === "ArrowUp"
                        ? 0.1
                        : -0.1),
                  );
                }
              }
            }}
            onChange={(event) => seek(Number(event.target.value))}
            aria-valuetext={`${Math.floor(time)} of ${Math.ceil(duration)} seconds`}
          />
        </div>
      </div>
      <figcaption className="ulo-visually-hidden">
        A recorded ulo session using GPT-5.6 Sol. A prompt is typed and
        submitted, then ulo reads a JavaScript project, edits its slug
        formatter, and runs the tests. Typing plays at double speed with a
        close-up that follows the cursor. Idle pauses are shortened. Click the
        video or use the play/pause button to control playback. Drag the
        timeline to seek; playback resumes when released. Space or Enter toggles
        playback when the video or timeline is focused. Arrow keys on the
        timeline seek. Reduced motion disables autoplay.
      </figcaption>
    </figure>
  );
}
