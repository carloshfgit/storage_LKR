<!-- Barra única do disco: categorias em ordem fixa à esquerda, "Livre" na ponta direita.
     Durante o scan sobra entre elas uma faixa listrada com o que ainda não foi contado;
     no fim do scan ela some, porque o restante entra em Apps/Sistema (DECISIONS D05). -->
<script lang="ts">
  import { cubicOut } from "svelte/easing";
  import { Tween, prefersReducedMotion } from "svelte/motion";
  import { fade } from "svelte/transition";
  import { CATEGORIES, type Category } from "./api";
  import type { Segment } from "./categories";
  import { formatBytes, formatPercent } from "./format";

  let {
    segments,
    free,
    total,
    scanning,
    onselect,
  }: {
    segments: Segment[];
    free: number;
    total: number;
    scanning: boolean;
    /** Ausente enquanto não há resultado completo: os segmentos não são clicáveis. */
    onselect?: (key: Category) => void;
  } = $props();

  // Abaixo disso o segmento não é desenhado, para não sobrar só o vão entre vizinhos.
  const MIN_PCT = 0.05;

  const options = () => ({ duration: prefersReducedMotion.current ? 0 : 600, easing: cubicOut });
  const widths = Object.fromEntries(CATEGORIES.map((c) => [c, new Tween(0, options())])) as Record<
    Category,
    Tween<number>
  >;
  const freeWidth = new Tween(0, options());

  const pct = (bytes: number) => (total > 0 ? (bytes / total) * 100 : 0);

  // Os eventos de progresso chegam a cada ~100 ms; cada um só redireciona a animação em curso.
  $effect(() => {
    for (const s of segments) widths[s.key].set(pct(s.bytes), options());
    freeWidth.set(pct(free), options());
  });

  // O que ainda não foi contado. Com o scan pronto é ~0, e a faixa sai junto com o seu vão.
  const pendingPct = $derived(
    100 - freeWidth.current - CATEGORIES.reduce((sum, c) => sum + widths[c].current, 0),
  );

  /** Tooltip do segmento sob o mouse; `x` é o centro dele, em px, relativo à barra. */
  let tip = $state<{ key: Category | "free"; x: number } | null>(null);

  function showTip(e: PointerEvent | FocusEvent) {
    const el = (e.target as HTMLElement).closest<HTMLElement>("[data-tip]");
    if (!el) return (tip = null);
    const box = el.getBoundingClientRect();
    const bar = el.parentElement!.getBoundingClientRect();
    tip = { key: el.dataset.tip as Category | "free", x: box.left - bar.left + box.width / 2 };
  }

  const tipText = $derived.by(() => {
    if (!tip) return "";
    const s = segments.find((seg) => seg.key === tip!.key);
    const [name, bytes] = s ? [s.label, s.bytes] : ["Livre", free];
    return `${name} · ${formatBytes(bytes)} · ${formatPercent(total > 0 ? bytes / total : 0)}`;
  });

  const label = $derived(
    [...segments.map((s) => `${s.label}: ${formatBytes(s.bytes)}`), `Livre: ${formatBytes(free)}`].join(", "),
  );
</script>

<div class="wrap">
  <!-- Os eventos ficam na barra: segmentos desabilitados não recebem eventos de mouse. -->
  <div
    class="bar"
    role="group"
    aria-label={label}
    onpointermove={showTip}
    onpointerleave={() => (tip = null)}
    onfocusin={showTip}
    onfocusout={() => (tip = null)}
  >
    {#each segments as s (s.key)}
      {#if widths[s.key].current > MIN_PCT}
        <button
          class="segment"
          data-tip={s.key}
          style:width="{widths[s.key].current}%"
          style:background={s.color}
          disabled={!onselect}
          onclick={() => onselect?.(s.key)}
          aria-label="{s.label}: {formatBytes(s.bytes)}"
        ></button>
      {/if}
    {/each}
    {#if pendingPct > MIN_PCT}
      <div class="pending" class:scanning></div>
    {/if}
    {#if freeWidth.current > MIN_PCT}
      <div class="segment free" data-tip="free" style:width="{freeWidth.current}%"></div>
    {/if}
  </div>

  {#if tip}
    <div class="tip" role="tooltip" style:--x="{tip.x}px" transition:fade={{ duration: 120 }}>{tipText}</div>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
  }

  /* Acima do segmento, sem sair da barra: o centro é limitado às bordas. */
  .tip {
    position: absolute;
    bottom: calc(100% + 8px);
    left: clamp(90px, var(--x), calc(100% - 90px));
    transform: translateX(-50%);
    padding: 5px 10px;
    border-radius: 8px;
    background: var(--text);
    color: var(--surface);
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    pointer-events: none;
    font-variant-numeric: tabular-nums;
    z-index: 1;
  }

  .bar {
    display: flex;
    gap: 2px;
    height: 28px;
    border-radius: var(--bar-radius);
    overflow: hidden;
    /* Aparece nos vãos de 2px: separa os segmentos como no iOS. */
    background: var(--surface);
  }

  .segment {
    flex: 0 1 auto;
    min-width: 0;
    padding: 0;
    border: none;
    border-radius: 0;
  }

  button.segment:enabled {
    cursor: pointer;
    transition: filter 0.15s var(--ease);
  }

  button.segment:enabled:hover {
    filter: brightness(1.12);
  }

  .free {
    background: var(--track);
  }

  /* Ainda não contado. Some (largura 0) quando o scan fecha os 100%. */
  .pending {
    flex: 1 1 0;
    min-width: 0;
    background: repeating-linear-gradient(
      -45deg,
      var(--track) 0 6px,
      var(--track-stripe) 6px 12px
    );
    background-size: 17px 17px;
  }

  .pending.scanning {
    animation: slide 0.8s linear infinite;
  }

  @keyframes slide {
    to {
      background-position: 17px 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .pending.scanning {
      animation: none;
    }
  }
</style>
