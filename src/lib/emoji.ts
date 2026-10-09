// Emoji data is ~600 KB, so it loads on first use instead of at startup.
export interface EmojiItem {
  emoji: string;
  name: string;
  shortcodes: string[];
  tags: string[];
  group: string;
}

let data: Promise<EmojiItem[]> | null = null;

// One emoji per Unicode emoji version, newest first. Each is a single code
// point, so an unsupported one renders as a monochrome box, not as pieces.
const SENTINELS: [number, string][] = [
  [16, "\u{1FAE9}"], // face with bags under eyes
  [15, "\u{1FAE8}"], // shaking face
  [14, "\u{1FAE0}"], // melting face
  [13, "\u{1F972}"], // smiling face with tear
  [12, "\u{1F971}"], // yawning face
  [11, "\u{1F970}"], // smiling face with hearts
];

/** Newest emoji version the OS font draws in colour; older systems lack recent emoji. */
function supportedVersion(): number {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 24;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return Infinity;
  ctx.font = "20px sans-serif";
  ctx.textBaseline = "top";
  for (const [version, emoji] of SENTINELS) {
    ctx.clearRect(0, 0, 24, 24);
    ctx.fillStyle = "#000";
    ctx.fillText(emoji, 0, 0);
    const px = ctx.getImageData(0, 0, 24, 24).data;
    for (let i = 0; i < px.length; i += 4) {
      // A coloured pixel means a real emoji glyph, not a black "tofu" box.
      if (px[i + 3] > 0 && Math.max(px[i], px[i + 1], px[i + 2]) - Math.min(px[i], px[i + 1], px[i + 2]) > 40) {
        return version;
      }
    }
  }
  return 5;
}

export function loadEmojis(): Promise<EmojiItem[]> {
  data ??= import("@tiptap/extension-emoji").then((m) => {
    const max = supportedVersion();
    return (m.emojis as (EmojiItem & { version: number })[]).filter(
      (e) => e.emoji && e.group !== "components" && e.version <= max,
    );
  });
  return data;
}

/** Best matches for a typed name: shortcode/name prefix first, then contains or tag prefix. */
export function matchEmojis(all: EmojiItem[], query: string, limit: number): EmojiItem[] {
  const q = query.toLowerCase().replace(/^:/, "").replace(/\s+/g, "_");
  if (!q) return all.slice(0, limit);
  const starts: EmojiItem[] = [];
  const contains: EmojiItem[] = [];
  for (const e of all) {
    if (e.shortcodes.some((s) => s.startsWith(q)) || e.name.startsWith(q)) starts.push(e);
    else if (contains.length < limit && (e.name.includes(q) || e.tags.some((t) => t.startsWith(q)))) contains.push(e);
    if (starts.length >= limit) break;
  }
  return starts.concat(contains).slice(0, limit);
}
