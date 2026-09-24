import script from "../../../../install.sh?raw";

export const prerender = false;

/** Publish the installer from this exact checkout alongside its documentation. */
export function GET(): Response {
  return new Response(script, {
    headers: {
      "Content-Type": "text/plain; charset=utf-8",
      "Cache-Control": "public, max-age=300",
      "X-Content-Type-Options": "nosniff",
    },
  });
}
