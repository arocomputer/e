/** Redirect bookmarks from the retired ulo domain to e's canonical site. */
export default {
  async fetch(request) {
    const incoming = new URL(request.url);
    const path = incoming.pathname.replace(/^\/(?:ulo|e)(?=\/|$)/, "") || "/";
    const shared = path === "/unsubscribe" || path.startsWith("/api/");
    const destination = new URL(shared ? path : `/e${path === "/" ? "" : path}`, "https://aro.computer");
    destination.search = incoming.search;
    return new Response("Redirecting...", {
      status: 308,
      headers: {
        location: destination.href,
        "cache-control": "public, max-age=3600",
        "x-content-type-options": "nosniff",
      },
    });
  },
};
