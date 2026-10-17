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
