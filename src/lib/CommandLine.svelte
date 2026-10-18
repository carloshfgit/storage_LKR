<!-- Um comando para o usuário rodar no terminal, selecionável e com "Copiar". -->
<script lang="ts">
  let { command }: { command: string } = $props();

  let copied = $state(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(command);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      // Sem permissão de área de transferência: o comando continua selecionável.
    }
  }
</script>

<div class="command">
  <code>{command}</code>
  <button onclick={copy} aria-label="Copiar comando">{copied ? "Copiado" : "Copiar"}</button>
</div>

<style>
  .command {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 6px;
    padding: 6px 6px 6px 10px;
    border-radius: 8px;
    background: var(--track);
  }

  code {
    flex: 1;
    overflow-x: auto;
    font-family: ui-monospace, "SF Mono", "DejaVu Sans Mono", monospace;
    font-size: 12px;
    white-space: nowrap;
    user-select: text;
  }

  button {
    flex: none;
    padding: 3px 12px;
    border: none;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
</style>
