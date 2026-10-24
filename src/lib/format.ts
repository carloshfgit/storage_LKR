// Formatação para a interface em pt-BR. Bytes em unidades decimais (1 GB = 10⁹ B), como o
// GNOME e o iOS (DECISIONS D20).

const UNITS = ["B", "kB", "MB", "GB", "TB", "PB"];

const numberFormats = new Map<number, Intl.NumberFormat>();

/** Casas decimais fixas: durante o scan os números não "pulam" de largura. */
function fixed(digits: number): Intl.NumberFormat {
  let f = numberFormats.get(digits);
  if (!f) {
    f = new Intl.NumberFormat("pt-BR", { minimumFractionDigits: digits, maximumFractionDigits: digits });
    numberFormats.set(digits, f);
  }
  return f;
}

/** Três algarismos significativos: "5,77 GB", "12,4 GB", "155 GB". */
export function formatBytes(bytes: number): string {
  let value = Math.max(0, bytes);
  let unit = 0;
  while (value >= 1000 && unit < UNITS.length - 1) {
    value /= 1000;
    unit++;
  }
  let digits = unit === 0 ? 0 : value < 10 ? 2 : value < 100 ? 1 : 0;
  // 999,7 MB arredondaria para "1.000 MB"; vira "1,00 GB".
  if (Number(value.toFixed(digits)) >= 1000 && unit < UNITS.length - 1) {
    value /= 1000;
    unit++;
    digits = 2;
  }
  return `${fixed(digits).format(value)} ${UNITS[unit]}`;
}

const percent = new Intl.NumberFormat("pt-BR", { style: "percent", maximumFractionDigits: 1 });

/** `fraction` de 0 a 1: "12,4%". Valores ínfimos viram "< 0,1%" em vez de "0%". */
export function formatPercent(fraction: number): string {
  if (fraction > 0 && fraction < 0.001) return `< ${percent.format(0.001)}`;
  return percent.format(fraction);
}

const count = new Intl.NumberFormat("pt-BR");

/** "1.070.000". */
export function formatCount(n: number): string {
  return count.format(n);
}

/** "2,3 s". */
export function formatSeconds(ms: number): string {
  return `${fixed(1).format(ms / 1000)} s`;
}

const relative = new Intl.RelativeTimeFormat("pt-BR", { numeric: "auto" });

const STEPS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["minute", 60],
  ["hour", 60],
  ["day", 24],
  ["month", 30],
  ["year", 12],
];

/** mtime em segundos Unix → "há 3 dias", "ontem", "há 2 anos". */
export function formatRelativeDate(unixSeconds: number, now = Date.now()): string {
  if (unixSeconds <= 0) return "—";
  let value = (unixSeconds * 1000 - now) / 1000;
  if (Math.abs(value) < 60) return "agora";
  let unit: Intl.RelativeTimeFormatUnit = "second";
  for (const [next, factor] of STEPS) {
    if (Math.abs(value) < factor) break;
    value /= factor;
    unit = next;
  }
  return relative.format(Math.round(value), unit);
}

/** Bytes por segundo: "12,4 MB/s", "0 B/s". */
export function formatRate(bytesPerSecond: number): string {
  return `${formatBytes(bytesPerSecond)}/s`;
}

/** "800 MHz", "2,40 GHz". */
export function formatMHz(mhz: number): string {
  if (mhz < 1000) return `${Math.round(mhz)} MHz`;
  return `${fixed(2).format(mhz / 1000)} GHz`;
}

/** "52 °C". */
export function formatTemp(celsius: number): string {
  return `${Math.round(celsius)} °C`;
}

/** Segundos → "3 d 4 h", "2 h 15 min", "45 min", "< 1 min". */
export function formatDuration(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  if (minutes < 1) return "< 1 min";
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);
  if (days > 0) return `${days} d ${hours % 24} h`;
  if (hours > 0) return `${hours} h ${minutes % 60} min`;
  return `${minutes} min`;
}
