import type { Paper, Feed } from "./types";

/** Allow only http(s) links; everything else collapses to a safe anchor. */
export function safeHref(url: string): string {
  return /^https?:\/\//i.test(url ?? "") ? url : "#";
}

function el(tag: string, className?: string, text?: string): HTMLElement {
  const n = document.createElement(tag);
  if (className) n.className = className;
  if (text != null) n.textContent = text;
  return n;
}

export function cardNode(p: Paper): HTMLElement {
  const card = el("div", "card");

  const titleWrap = el("div", "ctitle");
  const a = document.createElement("a");
  a.setAttribute("href", safeHref(p.link));
  a.target = "_blank";
  a.rel = "noopener";
  a.textContent = p.title;
  titleWrap.appendChild(a);
  card.appendChild(titleWrap);

  const meta = el("div", "meta");
  meta.appendChild(el("span", "badge", p.source));
  if (p.topic) meta.appendChild(el("span", "tlabel", p.topic));
  meta.appendChild(el("span", undefined, p.date_label || "—"));
  card.appendChild(meta);

  if (p.summary) card.appendChild(el("div", "summary", p.summary));
  return card;
}

export function renderFeed(container: HTMLElement, feed: Feed): void {
  container.replaceChildren();
  if (!feed.papers.length) {
    container.appendChild(el("div", "muted", "No papers found."));
    return;
  }
  for (const p of feed.papers) container.appendChild(cardNode(p));
}
