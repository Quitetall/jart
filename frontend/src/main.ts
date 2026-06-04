import { fetchFeed, summarize } from "./api";
import { renderFeed } from "./render";
import type { Paper } from "./types";

const PROMPT =
  "Synthesize the newest papers below into 2-3 short paragraphs: dominant themes, " +
  "anything new or surprising, and active directions. Ground claims only in the text. No preamble.";

function setMsg(target: HTMLElement, cls: string, text: string): void {
  target.replaceChildren();
  const d = document.createElement("div");
  d.className = cls;
  d.textContent = text;
  target.appendChild(d);
}

async function boot(): Promise<void> {
  const hero = document.getElementById("hero")!;
  const sumBody = document.getElementById("sumBody")!;
  setMsg(hero, "loading", "Loading papers…");
  try {
    const feed = await fetchFeed();
    renderFeed(hero, feed);
    document.getElementById("summarize")!.addEventListener("click", async () => {
      setMsg(sumBody, "loading", "Summarizing…");
      const items = feed.papers.slice(0, 14).map(
        (p: Paper) => `Title: ${p.title}\nAbstract: ${(p.grounding || p.summary).slice(0, 700)}`,
      );
      try {
        sumBody.textContent = await summarize(PROMPT, items);
      } catch (e) {
        setMsg(sumBody, "err", (e as Error).message);
      }
    });
  } catch (e) {
    setMsg(hero, "err", `Couldn't load feed: ${(e as Error).message}`);
  }
}
boot();
