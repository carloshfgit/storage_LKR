# storage_LKR

Gerenciador de armazenamento para Ubuntu no estilo iOS/Android: uma barra horizontal única dividida por cor, com categorias por tipo de arquivo e ações para liberar espaço com segurança.

```
[ Fotos | Vídeos | Áudio | Documentos | Apps/Sistema ··········· | Outros ····· | Livre          ]
```

- **Rápido:** escaneia `/` (~1 milhão de arquivos) em cerca de 2 s, com cache quente, sem root.
- **Preciso:** mede o espaço alocado em disco, como `df` e `du`. A barra sempre soma 100% do disco.
- **Classificação inteligente:** o caminho decide antes da extensão. Um ícone em `/usr` é Sistema, um JPG do cache do navegador é Outros e um AppImage é App.
- **Docker visível:** o espaço de `/var/lib/docker`, que só o root lê, é obtido do próprio Docker (`docker system df`) e aparece como subitem de Apps/Sistema.
- **Seguro (planejado):** nada é apagado direto. Os arquivos vão para a Lixeira do Ubuntu, e toda a validação acontece no Rust.

## Stack

Motor em **Rust** (`jwalk`, `statvfs`), janela **Tauri v2** e interface **SvelteKit (Svelte 5) + TypeScript** com CSS puro. A justificativa está em [STACK_CHOICE](docs/STACK_CHOICE.md).

## Começando

```bash
npm install
npm run tauri dev          # app em desenvolvimento
cargo test --workspace     # testes
```

O passo a passo completo, com pré-requisitos, a CLI de benchmark, o build do `.deb` e a resolução de problemas, está em **[docs/QUICK_SETUP.md](docs/QUICK_SETUP.md)**.

## Status

| Etapa | | |
|---|---|---|
| 0. Ambiente | ✅ | |
| 1. Motor de scan (CLI) | ✅ | Bate com `du` byte a byte em `$HOME` |
| 2. Classificação + Docker | ✅ | "Sem acesso" caiu de 106 GB para 9 GB |
| 3. Ponte Tauri | ✅ | Eventos `progress`/`done`/`docker` pelo Channel |
| 4. Tela principal | ✅ | Barra ao vivo, legenda e tema claro/escuro (falta a conferência visual) |
| 5. Detalhe da categoria | | |
| 6. Ações de limpeza | | |
| 7. Polimento e pacote `.deb` | | |

## Documentação

| Documento | Conteúdo |
|---|---|
| [QUICK_SETUP](docs/QUICK_SETUP.md) | Instalar, rodar, testar, empacotar |
| [PLAN](docs/PLAN.md) | Etapas, critérios de pronto e resultados medidos |
| [ARCHITECTURE](docs/ARCHITECTURE.md) | Como o motor, a ponte e a interface funcionam |
| [DECISIONS](docs/DECISIONS.md) | Cada decisão, com o porquê e o que foi descartado |
| [STACK_CHOICE](docs/STACK_CHOICE.md) | Por que Rust + Tauri + Svelte |
