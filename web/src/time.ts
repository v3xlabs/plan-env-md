import {
  format,
  formatDistanceToNow,
  isToday,
  isYesterday,
  parseISO,
  startOfWeek,
} from "date-fns";

// SQLite writes "YYYY-MM-DD HH:MM:SS" in UTC with no zone marker, so the space
// becomes a T and a Z is appended before parsing.
export const parseTimestamp = (timestamp: string) =>
  parseISO(`${timestamp.replace(" ", "T")}Z`);

export const dayLabel = (timestamp: string) => {
  const date = parseTimestamp(timestamp);

  if (isToday(date)) return "Today";

  if (isYesterday(date)) return "Yesterday";

  return format(date, "EEEE, MMMM d");
};

export const weekLabel = (timestamp: string) =>
  `Week of ${format(startOfWeek(parseTimestamp(timestamp), { weekStartsOn: 1 }), "MMM d")}`;

/// Absolute, for pinning down when something happened.
export const absolute = (timestamp: string) =>
  format(parseTimestamp(timestamp), "d MMM yyyy, HH:mm");

/// Relative, for judging how stale it is.
export const relative = (timestamp: string) =>
  formatDistanceToNow(parseTimestamp(timestamp), { addSuffix: true });

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/// Relative in a few characters, for the narrow time column of a row. Past a
/// week the date itself says more than a count of weeks.
export const compactRelative = (timestamp: string) => {
  const date = parseTimestamp(timestamp);
  const elapsed = Date.now() - date.getTime();

  if (elapsed < MINUTE) return "now";

  if (elapsed < HOUR) return `${Math.floor(elapsed / MINUTE)}m ago`;

  if (elapsed < DAY) return `${Math.floor(elapsed / HOUR)}h ago`;

  if (elapsed < 7 * DAY) return `${Math.floor(elapsed / DAY)}d ago`;

  return format(date, "d MMM");
};
