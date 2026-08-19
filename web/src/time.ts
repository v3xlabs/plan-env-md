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
