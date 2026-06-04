import type { Feed } from "./types";

// A stalled backend must not freeze the UI forever. AbortSignal.timeout is
// supported in modern browsers (and Node 18+).
const FEED_TIMEOUT_MS = 30_000;
const SUMMARY_TIMEOUT_MS = 90_000; // AI calls are slower than a feed fetch

export async function fetchFeed(): Promise<Feed> {
  const r = await fetch("/api/feed", { signal: AbortSignal.timeout(FEED_TIMEOUT_MS) });
  if (!r.ok) throw new Error(`feed ${r.status}`);
  return r.json();
}

export async function summarize(prompt: string, items: string[]): Promise<string> {
  const r = await fetch("/api/summary", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ prompt, items }),
    signal: AbortSignal.timeout(SUMMARY_TIMEOUT_MS),
  });
  // Parse the body first so we can surface the server's {error} message; fall
  // back to the HTTP status when the body is missing/non-JSON.
  let j: { text?: string; error?: string };
  try {
    j = await r.json();
  } catch {
    throw new Error(`summary ${r.status}`);
  }
  if (j.error) throw new Error(j.error);
  if (!r.ok) throw new Error(`summary ${r.status}`);
  return j.text as string;
}
