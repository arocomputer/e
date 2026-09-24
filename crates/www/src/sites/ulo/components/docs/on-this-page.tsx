import { useEffect, useRef, useState } from "react";

type Props = { sections: { id: string; title: string; depth: number }[] };

/** Track visible headings and lead their measured text range with a direction-aware dot. */
export function OnThisPage({ sections }: Props) {
  const [active, setActive] = useState(0);
  const [lastVisible, setLastVisible] = useState(0);
  const [current, setCurrent] = useState(0);
  const [line, setLine] = useState({
    path: "",
    height: 0,
    top: 0,
    bottom: 0,
    end: 0,
  });
  const list = useRef<HTMLOListElement>(null);

  useEffect(() => {
    let frame = 0;
    let mounted = true;
    let visible = sections.map(() => false);
    let first = 0;
    let last = 0;
    let hasScrolled = window.scrollY > 0;
    let previousRange: { first: number; last: number; up: boolean } | undefined;
    const headings = sections.map(({ id }) => document.getElementById(id));
    const measure = () => {
      frame = 0;
      hasScrolled ||= window.scrollY > 0;
      const initial = !hasScrolled;
      const returnedToTop = hasScrolled && window.scrollY <= 0;
      const up =
        returnedToTop ||
        (previousRange !== undefined &&
          (first < previousRange.first ||
            last < previousRange.last ||
            (first === previousRange.first &&
              last === previousRange.last &&
              previousRange.up)));
      previousRange = { first, last, up };
      let startIndex = initial || returnedToTop ? 0 : first;
      let endIndex = initial
        ? 0
        : returnedToTop
          ? Math.min(1, sections.length - 1)
          : last;
      // First load highlights only the first link; returning to the top keeps the connection.
      if (!initial && startIndex === endIndex && sections.length > 1) {
        if (up && endIndex < sections.length - 1) endIndex += 1;
        else if (startIndex > 0) startIndex -= 1;
        else endIndex += 1;
      }
      setActive(startIndex);
      setLastVisible(endIndex);
      setCurrent(initial || up ? startIndex : endIndex);
      const rows = Array.from(list.current?.children ?? []) as HTMLElement[];
      if (!rows.length) return;
      // Measure link text, excluding padding, and share one path for both strokes and the dot.
      const positions = rows.map((row, i) => {
        const link = row.querySelector("a")!;
        const style = getComputedStyle(link);
        const top = row.offsetTop + parseFloat(style.paddingTop);
        const bottom =
          row.offsetTop + link.clientHeight - parseFloat(style.paddingBottom);
        return { top, bottom, x: sections[i].depth === 3 ? 16.5 : 0.5 };
      });
      let path = "";
      let distance = 0;
      const lengths = positions.map(({ top, bottom, x }, i) => {
        if (i === 0) path = `M ${x} ${top}`;
        else {
          const previous = positions[i - 1];
          const middle = (previous.bottom + top) / 2;
          path += ` V ${middle} H ${x} V ${top}`;
          distance += top - previous.bottom + Math.abs(x - previous.x);
        }
        const start = distance;
        path += ` V ${bottom}`;
        distance += bottom - top;
        return { start, end: distance };
      });
      const height = positions.at(-1)!.bottom;
      const top = positions[startIndex].top;
      const bottom = height - positions[endIndex].bottom;
      const end =
        !initial && up ? lengths[startIndex].start : lengths[endIndex].end;
      setLine((previous) =>
        previous.path === path &&
        previous.height === height &&
        previous.top === top &&
        previous.bottom === bottom &&
        previous.end === end
          ? previous
          : { path, height, top, bottom, end },
      );
    };
    const schedule = () => {
      if (mounted && !frame) frame = requestAnimationFrame(measure);
    };
    const observer = new ResizeObserver(schedule);
    if (list.current) observer.observe(list.current);
    // Match the heading observer: intersecting headings win, then the nearest heading to the top.
    const headingsObserver = new IntersectionObserver(
      (entries) => {
        visible = visible.map((previous, i) => {
          const entry = entries.find(({ target }) => target === headings[i]);
          return entry ? entry.isIntersecting : previous;
        });
        first = visible.indexOf(true);
        last = visible.lastIndexOf(true);
        if (first === -1) {
          const top = entries[0]?.rootBounds?.top ?? 0;
          let nearest = Infinity;
          headings.forEach((heading, i) => {
            if (!heading) return;
            const distance = Math.abs(
              heading.getBoundingClientRect().top - top,
            );
            if (distance < nearest) {
              first = last = i;
              nearest = distance;
            }
          });
        }
        if (first !== -1) schedule();
      },
      { threshold: 0.9 },
    );
    headings.forEach((heading) => heading && headingsObserver.observe(heading));
    window.addEventListener("scroll", schedule, { passive: true });
    window.addEventListener("resize", schedule);
    void document.fonts.ready.then(schedule);
    return () => {
      mounted = false;
      cancelAnimationFrame(frame);
      observer.disconnect();
      headingsObserver.disconnect();
      window.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
    };
  }, [sections]);

  return (
    <aside className="ulo-page-index">
      <nav aria-label="On this page">
        <p>On this page</p>
        <div className="ulo-index-track">
          <svg
            className="ulo-index-base-line"
            aria-hidden="true"
            width="18"
            height={line.height}
          >
            <path d={line.path} />
          </svg>
          {line.path && (
            <>
              <svg
                className="ulo-index-active-line"
                aria-hidden="true"
                width="18"
                height={line.height}
                style={{
                  clipPath: `inset(${line.top}px 0 ${line.bottom}px 0)`,
                }}
              >
                <path d={line.path} />
              </svg>
              <span
                className="ulo-index-dot"
                aria-hidden="true"
                style={{
                  offsetPath: `path('${line.path}')`,
                  offsetDistance: `${line.end}px`,
                }}
              />
            </>
          )}
          <ol ref={list}>
            {sections.map(({ id, title, depth }, i) => (
              <li
                key={id}
                data-depth={depth}
                data-active={i >= active && i <= lastVisible}
              >
                <a
                  href={"#" + id}
                  aria-current={current === i ? "location" : undefined}
                >
                  {title}
                </a>
              </li>
            ))}
          </ol>
        </div>
      </nav>
    </aside>
  );
}
