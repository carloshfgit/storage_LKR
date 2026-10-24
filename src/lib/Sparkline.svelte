<!-- Gráfico de linha dos últimos `capacity` pontos, em SVG puro (DECISIONS D48). Os pontos novos
     entram pela direita. Escala fixa (`max`, para porcentagens) ou automática, com teto arredondado
     e nunca abaixo de `floor` (para taxas: o ruído de um disco parado não enche o gráfico). -->
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
    /** Formata o teto mostrado no canto (só na escala automática). */
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
    <line class="grid" x1="0" x2={capacity - 1} y1="50" y2="50" />
    <line class="base" x1="0" x2={capacity - 1} y1="100" y2="100" />
    {#each drawn as s (s.label)}
      <path class="area" d={s.area} fill={s.color} />
      <path class="line" d={s.line} stroke={s.color} />
    {/each}
  </svg>
  {#if format && max === undefined}
    <span class="ceiling">{format(ceiling)}</span>
  {/if}
</div>

<style>
  .chart {
    position: relative;
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

  .base {
    stroke: var(--separator);
    stroke-width: 1;
  }

  .ceiling {
    position: absolute;
    top: -2px;
    right: 0;
    color: var(--text-secondary);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
</style>
