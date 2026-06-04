import { describe, it, expect } from "vitest";
import { cardNode, safeHref } from "./render";
import type { Paper } from "./types";

const paper: Paper = {
  kind: "paper", source: "HF", topic: "Foundation models",
  title: "An <EEG> Model", link: "https://hf.co/papers/1",
  date_label: "2026-05-01", ts: 1, summary: "short", grounding: "g",
};

describe("render", () => {
  it("treats titles as text, not markup (no injection)", () => {
    const node = cardNode(paper);
    const a = node.querySelector("a")!;
    // textContent preserves the literal angle brackets; nothing is parsed as a tag
    expect(a.textContent).toBe("An <EEG> Model");
    expect(node.querySelector("EEG")).toBeNull();
  });
  it("renders source badge, topic, and link href", () => {
    const node = cardNode(paper);
    expect(node.querySelector(".badge")!.textContent).toBe("HF");
    expect(node.querySelector(".tlabel")!.textContent).toBe("Foundation models");
    expect(node.querySelector("a")!.getAttribute("href")).toBe("https://hf.co/papers/1");
  });
  it("rejects javascript: and data: scheme hrefs", () => {
    expect(safeHref("javascript:alert(1)")).toBe("#");
    expect(safeHref("data:text/html,<script>alert(1)</script>")).toBe("#");
    expect(safeHref("https://ok.com")).toBe("https://ok.com");
    expect(safeHref("http://ok.com")).toBe("http://ok.com");
  });
});
