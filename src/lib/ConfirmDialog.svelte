<!-- Diálogo modal de confirmação. Aparece enquanto `request` não for nulo; Esc ou
     "Cancelar" chamam `oncancel`. -->
<script lang="ts" module>
  export interface ConfirmRequest {
    title: string;
    message: string;
    confirmLabel: string;
  }
</script>

<script lang="ts">
  let {
    request,
    onconfirm,
    oncancel,
  }: { request: ConfirmRequest | null; onconfirm: () => void; oncancel: () => void } = $props();

  let dialog: HTMLDialogElement;

  $effect(() => {
    if (request && !dialog.open) dialog.showModal();
    else if (!request && dialog.open) dialog.close();
  });
</script>

<!-- `cancel` é o Esc: o fechamento passa pelo estado, não pelo navegador. -->
<dialog
  bind:this={dialog}
  oncancel={(e) => {
    e.preventDefault();
    oncancel();
  }}
>
  {#if request}
    <h2>{request.title}</h2>
    <p>{request.message}</p>
    <div class="actions">
      <button onclick={oncancel}>Cancelar</button>
      <button class="destructive" onclick={onconfirm}>{request.confirmLabel}</button>
    </div>
  {/if}
</dialog>

<style>
  dialog {
    width: min(380px, calc(100vw - 48px));
    padding: 20px 20px 16px;
    border: none;
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
  }

  dialog::backdrop {
    background: rgba(0, 0, 0, 0.35);
  }

  h2 {
    margin: 0 0 6px;
    font-size: 17px;
    font-weight: 600;
  }

  p {
    margin: 0 0 18px;
    color: var(--text-secondary);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  button {
    padding: 6px 16px;
    border: none;
    border-radius: 999px;
    background: var(--track);
    color: var(--text);
    font-weight: 600;
    cursor: pointer;
  }

  .destructive {
    background: var(--danger);
    color: #fff;
  }
</style>
