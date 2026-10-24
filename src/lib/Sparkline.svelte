<!-- Gráfico de linha dos últimos `capacity` pontos, em SVG puro (DECISIONS D48). Os pontos novos
     entram pela direita. Escala fixa (`max`, para porcentagens) ou automática, com teto arredondado
     e nunca abaixo de `floor` (para taxas: o ruído de um disco parado não enche o gráfico).
     Grade a cada 25% da escala e a cada 15 pontos (15 s), com o topo e o meio rotulados à direita. -->
<script lang="ts" module>
  export interface Series {
    label: string;
    color: string;
    /** Do mais antigo ao mais novo; `null` deixa um buraco na linha. */
    values: (number | null)[];
  }

  /** 1, 2 ou 5 × 10ⁿ acima de `v`. */
  export function niceCeil(v: number): number {
    if (v <= 0) return 0;
    const exp = 10 ** Math.floor(Math.log10(v));
    const m = v / exp;
    return (m <= 1 ? 1 : m <= 2 ? 2 : m <= 5 ? 5 : 10) * exp;
  }
</script>

<script lang="ts">
  import { formatPercent } from "./format";

  let {
    series,
    capacity = 60,
    max,
    floor = 1,
    format,
    height = 64,
    label,
  }: {
    series: Series[];
    capacity?: number;
    max?: number;
    floor?: number;
    /** Formata os rótulos da escala; sem ele, em % de `max`. */
    format?: (v: number) => string;
    height?: number;
    /** Resumo para o leitor de tela. */
    label: string;
  } = $props();

  const ceiling = $derived.by(() => {
    if (max !== undefined) return max;
    let peak = 0;
    for (const s of series) for (const v of s.values) if (v !== null && v > peak) peak = v;
    return Math.max(niceCeil(peak), floor);
  });

  /** Linhas horizontais (em % da altura, 0 = topo) e verticais (a cada 15 pontos, a partir do mais novo). */
  const ROWS = [0, 25, 50, 75];
  const cols = $derived(
    Array.from({ length: Math.ceil((capacity - 1) / 15) - 1 }, (_, i) => capacity - 1 - 15 * (i + 1)),
  );

  const tick = (v: number) => (format ? format(v) : formatPercent(v / (max || 1)));

  /** Linha e área de uma série, cortadas nos `null`. */
  function paths(values: (number | null)[]): { line: string; area: string } {
    const offset = capacity - values.length;
    let line = "";
    let area = "";
    let run: [number, number][] = [];
    const flush = () => {
      if (run.length === 0) return;
      const pts = run.map(([x, y]) => `${x} ${y.toFixed(2)}`).join(" L ");
      line += `M ${pts} `;
      area += `M ${run[0][0]} 100 L ${pts} L ${run[run.length - 1][0]} 100 Z `;
      run = [];
    };
    values.forEach((v, i) => {
      if (v === null) return flush();
      const y = 100 - (Math.min(v, ceiling) / (ceiling || 1)) * 100;
      run.push([offset + i, y]);
    });
    flush();
    return { line, area };
  }

  const drawn = $derived(series.map((s) => ({ ...s, ...paths(s.values) })));
</script>

<div class="chart" style:height="{height}px">
  <svg viewBox="0 0 {capacity - 1} 100" preserveAspectRatio="none" role="img" aria-label={label}>
    {#each cols as x (x)}
      <line class="grid minor" x1={x} x2={x} y1="0" y2="100" />
    {/each}
    {#each ROWS as y (y)}
      <line class="grid" class:minor={y % 50 !== 0} x1="0" x2={capacity - 1} y1={y} y2={y} />
    {/each}
    <line class="base" x1="0" x2={capacity - 1} y1="100" y2="100" />
    {#each drawn as s (s.label)}
      <path class="area" d={s.area} fill={s.color} />
      <path class="line" d={s.line} stroke={s.color} />
    {/each}
  </svg>
  {#if format || max !== undefined}
    <div class="scale" aria-hidden="true">
      <span>{tick(ceiling)}</span>
      <span>{tick(ceiling / 2)}</span>
    </div>
  {/if}
</div>

<style>
  /* O gráfico e, à direita, a coluna da escala: os rótulos não cobrem os pontos mais novos. */
  .chart {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    column-gap: 6px;
    width: 100%;
  }

  svg {
    display: block;
    width: 100%;
    height: 100%;
    overflow: visible;
  }

  /* A escala do viewBox é distorcida; o traço não, para ficar nítido em qualquer tamanho e em HiDPI. */
  line,
  .line {
    vector-effect: non-scaling-stroke;
  }

  .line {
    fill: none;
    stroke-width: 1.6;
    stroke-linejoin: round;
    stroke-linecap: round;
  }

  .area {
    opacity: 0.14;
  }

  .grid {
    stroke: var(--separator);
    stroke-dasharray: 3 3;
    stroke-width: 1;
  }

  .minor {
    stroke-dasharray: 1 3;
    opacity: 0.7;
  }

  .base {
    stroke: var(--separator);
    stroke-width: 1;
  }

  /* Linhas de altura zero no topo e no meio: cada rótulo fica centrado na sua linha da grade. */
  .scale {
    display: grid;
    grid-template-rows: 0 1fr 0 1fr;
    align-items: center;
    justify-items: end;
    color: var(--text-secondary);
    font-size: 10px;
    line-height: 1;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .scale span:last-child {
    grid-row: 3;
  }
</style>
