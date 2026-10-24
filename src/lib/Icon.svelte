<!-- Ícones de traço (24×24, cor do texto), no mesmo estilo do ícone da Lixeira: seletor de abas e
     títulos dos cards da aba Recursos. Sempre decorativos: o texto ao lado já diz o que são. -->
<script lang="ts" module>
  export type IconName = "drive" | "gauge" | "cpu" | "memory" | "network" | "gpu" | "processes" | "computer";
</script>

<script lang="ts">
  let { name, size = 16 }: { name: IconName; size?: number } = $props();
</script>

<svg viewBox="0 0 24 24" width={size} height={size} aria-hidden="true">
  {#if name === "drive"}
    <!-- HD/SSD: gabinete com o separador e os LEDs. -->
    <path d="M6.2 4h11.6l3.2 8v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-6z" />
    <path d="M3 12h18M7 16h.01M10.5 16h.01M15 16h3" />
  {:else if name === "gauge"}
    <!-- Velocímetro: arco de 270° com marcas e o ponteiro. -->
    <path d="M5.6 19.4a9 9 0 1 1 12.8 0" />
    <path d="M12 5v1.5M4.5 13H6M18 13h1.5M6.7 7.7l1.1 1.1" />
    <path d="m12 13 3.5-4.5" />
    <circle cx="12" cy="13" r="1.5" fill="currentColor" />
  {:else if name === "cpu"}
    <rect x="6" y="6" width="12" height="12" rx="2" />
    <rect x="9.5" y="9.5" width="5" height="5" rx="0.8" />
    <path d="M10 3v3M14 3v3M10 18v3M14 18v3M3 10h3M3 14h3M18 10h3M18 14h3" />
  {:else if name === "memory"}
    <!-- Pente de RAM: chips e contatos embaixo. -->
    <rect x="2.5" y="6" width="19" height="10" rx="1.5" />
    <path d="M6.5 9v4M10.5 9v4M13.5 9v4M17.5 9v4M5 16v3M8.5 16v3M12 16v3M15.5 16v3M19 16v3" />
  {:else if name === "network"}
    <!-- Download e upload, como no card. -->
    <path d="M8 4v15M4.5 15.5 8 19l3.5-3.5M16 20V5M12.5 8.5 16 5l3.5 3.5" />
  {:else if name === "gpu"}
    <!-- Placa de vídeo: ventoinha, saídas e o conector. -->
    <rect x="2.5" y="5" width="19" height="11" rx="1.5" />
    <circle cx="9" cy="10.5" r="3" />
    <path d="M15 8.5h3.5M15 12.5h3.5M6 16v3h9v-3" />
  {:else if name === "processes"}
    <!-- Ranking: linhas que encurtam. -->
    <path d="M4 6h.01M4 12h.01M4 18h.01M8.5 6H20M8.5 12h8.5M8.5 18H14" />
  {:else if name === "computer"}
    <rect x="3" y="4" width="18" height="12" rx="1.5" />
    <path d="M8.5 20h7M12 16v4" />
  {/if}
</svg>

<style>
  svg {
    flex-shrink: 0;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
