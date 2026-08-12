export interface SkillCardDatePresentation {
  short: string;
  full: string;
}

export function formatSkillCardDate(
  isoString: string,
  locale?: string,
  now = new Date(),
): SkillCardDatePresentation {
  const date = new Date(isoString);
  if (Number.isNaN(date.getTime())) {
    return { short: isoString, full: isoString };
  }

  const sameDay = date.getFullYear() === now.getFullYear()
    && date.getMonth() === now.getMonth()
    && date.getDate() === now.getDate();
  const shortOptions: Intl.DateTimeFormatOptions = sameDay
    ? { hour: '2-digit', minute: '2-digit', hour12: false }
    : date.getFullYear() === now.getFullYear()
      ? { month: 'short', day: 'numeric' }
      : { year: 'numeric', month: 'short', day: 'numeric' };

  return {
    short: new Intl.DateTimeFormat(locale, shortOptions).format(date),
    full: new Intl.DateTimeFormat(locale, {
      dateStyle: 'long',
      timeStyle: 'short',
    }).format(date),
  };
}
