import { useEffect, useRef, useState } from "react";

type Result = { url: string; title: string; page: string; excerpt: string };
type Entry = { label: string; links: [string, string][] };
const pagesFrom = (groups: Entry[]): Result[] =>
  groups.flatMap((group) =>
    group.links.map(([url, title]) => ({
      url,
      title,
      page: title,
      excerpt: "",
    })),
  );
type SearchData = {
  url: string;
  meta: { title: string };
  plain_excerpt: string;
  sub_results: { url: string; title: string; plain_excerpt: string }[];
};
type Pagefind = {
  options: (options: { baseUrl: string }) => Promise<void>;
  search: (
    query: string,
  ) => Promise<{ results: { data: () => Promise<SearchData> }[] }>;
};
let engine: Promise<Pagefind> | undefined;
let loadAttempt = 0;

/** Load the generated search bundle only when a visitor searches; allow retry after failure. */
function loadSearch() {
  const path = `/ulo/pagefind/pagefind.js${loadAttempt ? `?retry=${loadAttempt}` : ""}`;
  engine ??= import(/* @vite-ignore */ path)
    .then(async (search: Pagefind) => {
      await search.options({ baseUrl: "/" });
      return search;
    })
    .catch((error) => {
      engine = undefined;
      loadAttempt += 1;
      throw error;
    });
  return engine;
}

/** Render Pagefind's escaped plain excerpt as text, never as executable result HTML. */
function decodeExcerpt(text: string) {
  const element = document.createElement("textarea");
  element.innerHTML = text;
  return element.value;
}

/** Native modal search with shortcut access, keyboard result navigation, and focus restoration. */
export function DocsSearch({ groups }: { groups: Entry[] }) {
  const pages = pagesFrom(groups);
  const dialog = useRef<HTMLDialogElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const restoreFocus = useRef<HTMLElement | null>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Result[]>([]);
  const [active, setActive] = useState(0);
  const [state, setState] = useState<"idle" | "loading" | "ready" | "error">(
    "idle",
  );
  const [attempt, setAttempt] = useState(0);

  function show() {
    if (!dialog.current?.open) {
      restoreFocus.current = document.activeElement as HTMLElement;
      dialog.current?.showModal();
      setOpen(true);
    }
    input.current?.focus();
  }

  useEffect(() => {
    const shortcut = (event: KeyboardEvent) => {
      if (
        (event.metaKey || event.ctrlKey) &&
        event.key.toLowerCase() === "k" &&
        !event.altKey
      ) {
        event.preventDefault();
        if (!dialog.current?.open) {
          restoreFocus.current = document.activeElement as HTMLElement;
          dialog.current?.showModal();
          setOpen(true);
        }
        input.current?.focus();
      }
    };
    window.addEventListener("keydown", shortcut);
    return () => window.removeEventListener("keydown", shortcut);
  }, []);

  useEffect(() => {
    if (!open) return;
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = previous;
    };
  }, [open]);

  useEffect(() => {
    if (!open || !query.trim()) return;
    let cancelled = false;
    const timer = setTimeout(async () => {
      try {
        const search = await loadSearch();
        const response = await search.search(query.trim());
        const pages = await Promise.all(
          response.results.slice(0, 8).map((result) => result.data()),
        );
        if (cancelled) return;
        const matches = pages
          .flatMap((page) => {
            const sections = page.sub_results?.length
              ? page.sub_results
              : [
                  {
                    url: page.url,
                    title: page.meta.title,
                    plain_excerpt: page.plain_excerpt,
                  },
                ];
            return sections.slice(0, 3).map((section) => ({
              url: section.url,
              title: section.title,
              page: page.meta.title,
              excerpt: decodeExcerpt(section.plain_excerpt),
            }));
          })
          .filter(
            (result) =>
              result.url.startsWith("/ulo/docs") &&
              !result.url.startsWith("//"),
          )
          .slice(0, 12);
        setResults(matches);
        setActive(0);
        setState("ready");
      } catch {
        if (!cancelled) {
          setResults([]);
          setState("error");
        }
      }
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [query, open, attempt]);

  useEffect(() => {
    if (open)
      document
        .getElementById(`doc-result-${active}`)
        ?.scrollIntoView({ block: "nearest", behavior: "instant" });
  }, [active, open]);

  function choose(result: Result) {
    dialog.current?.close();
    window.location.assign(result.url);
  }

  const visibleResults = !query.trim()
    ? pages
    : state === "ready"
      ? results
      : [];
  const count = visibleResults.length;
  return (
    <>
      <button
        className="ulo-search-trigger"
        onClick={show}
        aria-haspopup="dialog"
        aria-label="Search documentation"
        aria-keyshortcuts="Meta+K Control+K"
      >
        <span>Search</span>
        <kbd>⌘K</kbd>
      </button>
      <dialog
        ref={dialog}
        className="ulo-search-dialog"
        aria-label="Search documentation"
        onClose={() => {
          setOpen(false);
          restoreFocus.current?.focus();
        }}
        onClick={(event) => {
          if (event.target === event.currentTarget) dialog.current?.close();
        }}
      >
        <div className="ulo-search-panel">
          <div className="ulo-search-input-row">
            <input
              ref={input}
              type="text"
              inputMode="search"
              enterKeyHint="search"
              role="combobox"
              aria-label="Search documentation"
              aria-expanded={count > 0}
              aria-controls="doc-search-results"
              aria-autocomplete="list"
              aria-activedescendant={count ? `doc-result-${active}` : undefined}
              placeholder="Search documentation…"
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setResults([]);
                setActive(0);
                setState(event.target.value.trim() ? "loading" : "idle");
              }}
              onKeyDown={(event) => {
                if (!count || event.nativeEvent.isComposing) return;
                if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                  event.preventDefault();
                  setActive(
                    (current) =>
                      (current + (event.key === "ArrowDown" ? 1 : count - 1)) %
                      count,
                  );
                }
                if (event.key === "Enter") {
                  event.preventDefault();
                  choose(visibleResults[active]);
                }
              }}
            />
            <button
              className="ulo-search-close"
              onClick={() => dialog.current?.close()}
              aria-label="Close search"
            >
              Esc
            </button>
          </div>
          <div className="ulo-search-body">
            <p className="ulo-search-status" role="status">
              {state === "idle"
                ? "Pages"
                : state === "loading"
                  ? "Searching…"
                  : state === "error"
                    ? "Search is unavailable. Try loading the index again."
                    : count
                      ? `${count} ${count === 1 ? "result" : "results"}`
                      : `No results for “${query}”.`}
            </p>
            {state === "error" && (
              <button
                className="ulo-search-retry"
                onClick={() => {
                  setState("loading");
                  setAttempt((value) => value + 1);
                }}
              >
                Retry
              </button>
            )}
            <ul
              id="doc-search-results"
              role="listbox"
              aria-label="Search results"
            >
              {visibleResults.map((result, i) => (
                <li
                  key={result.url}
                  id={`doc-result-${i}`}
                  role="option"
                  aria-selected={active === i}
                  onMouseMove={() => setActive(i)}
                  onClick={() => choose(result)}
                >
                  {result.page !== result.title && (
                    <span className="ulo-search-page">{result.page}</span>
                  )}
                  <strong>{result.title}</strong>
                  {result.excerpt && <p>{result.excerpt}</p>}
                </li>
              ))}
            </ul>
          </div>
        </div>
      </dialog>
    </>
  );
}
