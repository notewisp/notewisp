// Times are stored as UTC milliseconds and shown in local time.

const DAY_MS = 86_400_000;

export function dayKey(ms: number): number {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

const weekdayDay = new Intl.DateTimeFormat(undefined, { weekday: "short", day: "numeric", month: "short" });
const withYear = new Intl.DateTimeFormat(undefined, {
  weekday: "short",
  day: "numeric",
  month: "short",
  year: "numeric",
});
const clock = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });
const full = new Intl.DateTimeFormat(undefined, { dateStyle: "full", timeStyle: "short" });

/** "Today", "Yesterday", "Mon 6 Oct", or with the year if not this year. */
export function dayLabel(ms: number, now = Date.now()): string {
  const day = dayKey(ms);
  const today = dayKey(now);
  if (day === today) return "Today";
  // Round: DST days are 23 or 25 hours long.
  if (Math.round((today - day) / DAY_MS) === 1) return "Yesterday";
  const sameYear = new Date(ms).getFullYear() === new Date(now).getFullYear();
  return (sameYear ? weekdayDay : withYear).format(ms).replace(",", "");
}

export function timeLabel(ms: number): string {
  return clock.format(ms);
}

export function fullLabel(ms: number): string {
  return full.format(ms);
}
