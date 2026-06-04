import type { Feed } from "./types";

export async function fetchFeed(): Promise<Feed> {
  const r = await fetch("/api/feed");
  if (!r.ok) throw new Error(`feed ${r.status}`);
  return r.json();
}

export async function summarize(prompt: string, items: string[]): Promise<string> {
  const r = await fetch("/api/summary", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ prompt, items }),
  });
  const j = await r.json();
  if (j.error) throw new Error(j.error);
  return j.text as string;
}
