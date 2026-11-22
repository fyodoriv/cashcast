const cad = new Intl.NumberFormat("en-CA", { style: "currency", currency: "CAD", maximumFractionDigits: 0 });
const short = new Intl.NumberFormat("en-CA", { notation: "compact", maximumFractionDigits: 1 });

export const money = (x: number | null | undefined) => (x === null || x === undefined ? "—" : cad.format(x));
/** Axis labels: $12k, -$3.5k. */
export const compact = (x: number) => `${x < 0 ? "-" : ""}$${short.format(Math.abs(x))}`;
export const pct = (x: number | null | undefined, digits = 1) =>
  x === null || x === undefined ? "—" : `${(x * 100).toFixed(digits)}%`;
/** "neg" for amounts below zero, so they show red. */
export const sign = (x: number | null | undefined) => (x !== null && x !== undefined && x < 0 ? "neg" : undefined);

export function monthLabel(ym: string): string {
  const [y, m] = ym.split("-").map(Number);
  return new Date(y, m - 1, 1).toLocaleDateString("en-CA", { month: "short", year: "numeric" });
}

/** "Oct 16" from YYYY-MM-DD. */
export function dayLabel(iso: string): string {
  const [y, m, d] = iso.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString("en-CA", { month: "short", day: "numeric" });
}

export const monthOf = (isoDate: string) => isoDate.slice(0, 7);

/** "Sun, Oct 18" from YYYY-MM-DD: a date people read, never ISO. */
export function weekdayLabel(iso: string): string {
  const [y, m, d] = iso.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString("en-CA", { weekday: "short", month: "short", day: "numeric" });
}

/** Rewrites every YYYY-MM-DD in engine text as "Apr 30, 2027". */
export function friendlyDates(text: string): string {
  return text.replace(/\b(\d{4})-(\d{2})-(\d{2})\b/g, (whole, y, m, d) => {
    const date = new Date(Number(y), Number(m) - 1, Number(d));
    // A date that rolls over (2027-13-40) is not a real date: leave the text as it is.
    if (date.getMonth() !== Number(m) - 1 || date.getDate() !== Number(d)) return whole;
    return date.toLocaleDateString("en-CA", { month: "short", day: "numeric", year: "numeric" });
  });
}
