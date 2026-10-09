// Turns a note's cached HTML into what the timeline shows: sanitised with
// DOMPurify, with #tags turned into clickable chips. Results are cached so
// scrolling never re-sanitises a note.
import DOMPurify from "dompurify";
import { splitTags } from "./tags";
import type { Note } from "./api";

const CACHE_MAX = 3000;
const cache = new Map<string, string>();

DOMPurify.addHook("afterSanitizeAttributes", (node) => {
  if (node.tagName === "A") {
    node.removeAttribute("target");
    node.setAttribute("rel", "noopener noreferrer");
  }
  if (node.tagName === "INPUT") node.setAttribute("disabled", "");
});

// Text inside these is never a tag.
const SKIP = new Set(["A", "CODE", "PRE", "BUTTON"]);

function chipTags(root: DocumentFragment) {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      for (let p = node.parentElement; p; p = p.parentElement) {
        if (SKIP.has(p.tagName)) return NodeFilter.FILTER_REJECT;
      }
      return node.nodeValue?.includes("#") ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP;
    },
  });
  const nodes: Text[] = [];
  while (walker.nextNode()) nodes.push(walker.currentNode as Text);
  for (const node of nodes) {
    const parts = splitTags(node.nodeValue!);
    if (!parts.some((p) => p.tag)) continue;
    const frag = document.createDocumentFragment();
    for (const part of parts) {
      if (!part.tag) {
        frag.append(part.text);
        continue;
      }
      const chip = document.createElement("button");
      chip.type = "button";
      chip.className = "tag";
      chip.tabIndex = -1;
      chip.dataset.tag = part.tag;
      chip.textContent = part.text;
      frag.append(chip);
    }
    node.replaceWith(frag);
  }
}

export function renderNote(note: Note): string {
  const key = `${note.id}:${note.updated_at}`;
  let html = cache.get(key);
  if (html !== undefined) return html;
  const frag = DOMPurify.sanitize(note.html, {
    RETURN_DOM_FRAGMENT: true,
    FORBID_TAGS: ["style", "img", "video", "audio", "iframe", "form"],
    FORBID_ATTR: ["class", "id"],
    ALLOW_DATA_ATTR: true,
  });
  chipTags(frag);
  const div = document.createElement("div");
  div.append(frag);
  html = div.innerHTML;
  if (cache.size >= CACHE_MAX) cache.delete(cache.keys().next().value!);
  cache.set(key, html);
  return html;
}
