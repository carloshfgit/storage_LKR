# STACK_CHOICE — Escolha de Tecnologia

> App local para Ubuntu que mostra o armazenamento como o iOS/Android: uma barra segmentada por cor, categorias por tipo de arquivo e varredura rápida.

## TL;DR

| Camada | Escolha |
|---|---|
| Motor (scan, classificação, lixeira) | **Rust** (stable, edition 2021) |
| Shell desktop | **Tauri v2** (WebKitGTK 4.1) |
| Interface | **Svelte 5 + TypeScript + Vite** |
| Estilo | **CSS puro**, com custom properties (sem Tailwind e sem lib de componentes) |
| Empacotamento | `.deb` gerado por `tauri build` |

---

## 1. Rust é uma boa ideia? **Sim. Mantido.**

Não há motivo para refutar. Para este problema específico, Rust é a melhor opção:

| Necessidade | Como o Rust atende |
|---|---|
| Escanear cerca de 1 milhão de entradas rápido | `jwalk` faz o walk paralelo em todos os núcleos. Não há GIL, e o I/O de metadados (`statx`) roda em paralelo de verdade |
| Medir espaço real em disco (como o `df`) | Acesso direto a `st_blocks`, `st_dev`, `st_ino` e `st_nlink` via `std::os::unix::fs::MetadataExt` |
| Fechar a conta com o tamanho do disco | `statvfs` via `rustix`, sem precisar fazer parse da saída de `df` |
| Mandar para a Lixeira do Ubuntu | O crate `trash` implementa a especificação FreeDesktop Trash, então o arquivo pode ser restaurado pelo Nautilus |
| App leve e sempre pronto | Binário nativo, poucos MB de RAM no motor e nenhum runtime para instalar |

**Custos assumidos:**
- **Curva de aprendizado:** ownership e borrow checker. O motor é pequeno (cerca de 5 módulos), e o código Rust fica concentrado e isolado num crate testável.
- **Compilação:** o primeiro build do Tauri leva vários minutos. Os builds incrementais levam segundos.
- **Toolchain:** Rust **não está instalado** nesta máquina. É preciso rodar o `rustup` (Etapa 0 do PLAN).

**Por que não Python no motor:** `os.scandir` é rápido por arquivo, mas o walk fica preso a uma thread por causa do GIL. Em cerca de 1 milhão de entradas, a diferença costuma ser de 3 a 5 vezes. Python seria ótimo para um protótipo, mas o requisito 3 ("velocidade") pesa a favor do Rust.

---

## 2. Stack visual: **Tauri v2 + Svelte 5**

O requisito 1 (barra horizontal única dividida por cores, visual limpo) é, na prática, um problema de **CSS**:

```css
.bar      { display:flex; height:28px; border-radius:14px; overflow:hidden; background:var(--track); }
.segment  { flex-basis: calc(var(--pct) * 1%); background: var(--color); transition: flex-basis .6s cubic-bezier(.2,.8,.2,1); }
```

Com toolkits nativos, a mesma barra exige desenho manual (Cairo/Canvas) e animação feita à mão.

### Por que Tauri v2
- O backend **é** Rust: o motor é chamado direto, sem processo extra nem IPC por HTTP.
- Usa o WebKitGTK do sistema, então a janela ocupa cerca de 30 a 50 MB de RAM, contra 150 a 300 MB do Electron.
- `tauri::ipc::Channel` serve para streaming de progresso eficiente durante o scan.
- O sistema de **capabilities** permite expor ao frontend só os comandos necessários.
- `tauri build` gera o `.deb` com a entrada `.desktop`.
- Nesta máquina (Wayland + Intel HD 620) não há os problemas conhecidos do WebKitGTK com drivers NVIDIA.

### Por que Svelte 5 (e não React/Vue)
- Compila para JS mínimo, sem virtual DOM, o que dá um bundle pequeno e uma abertura instantânea.
- `svelte/motion` (`tweened`/`Spring`) e `svelte/transition` vêm prontos, então a barra "cresce" suavemente enquanto o scan avança.
- A sintaxe é próxima de HTML puro e tem menos boilerplate para um app de 4 ou 5 componentes.

### Por que CSS puro
- O app tem poucos componentes. Custom properties (`--cat-photos`, `--radius`) e `prefers-color-scheme` resolvem o tema claro/escuro.
- Tailwind ou uma lib de UI acrescentariam dependência sem ganho real.

---

## 3. Alternativas avaliadas e rejeitadas

| Alternativa | Por que não |
|---|---|
| **GTK4 + libadwaita** (gtk4-rs / relm4) | Integra perfeitamente ao GNOME, mas a barra segmentada custom, as animações e o visual "iOS" exigem `DrawingArea` + Cairo e muito código de UI imperativo |
| **Slint** | Bom e leve, mas toda a estilização é manual e o ecossistema é menor |
| **iced** | Arquitetura Elm elegante, mas exige estilização manual e anima pouco por padrão |
| **egui** | Modo imediato com cara de ferramenta de debug, longe do visual iOS/Android |
| **Electron** | Visual idêntico ao Tauri, porém pesado (Chromium embutido e mais de 150 MB de RAM) |
| **Python + PyWebView / PySide6** | Motor mais lento (GIL), e o Qt dá trabalho para ficar com cara de iOS |
| **Dioxus** | Também renderiza em webview no desktop e é menos maduro que o Tauri para empacotamento e permissões |

---

## 4. Dependências previstas

### Rust (`crates/scanner` e `src-tauri`)
| Crate | Uso |
|---|---|
| `jwalk` | Walk paralelo de diretórios (rayon por baixo) |
| `rustix` (feature `fs`) | `statvfs` (total, livre e disponível do filesystem) |
| `trash` | Mover para a Lixeira e esvaziá-la (FreeDesktop) |
| `serde` + `serde_json` | Serialização para o frontend e para o cache do último scan |
| `thiserror` | Erros tipados |
| `tauri` v2 | Shell desktop, commands e `ipc::Channel` |
| `tauri-plugin-opener` | `reveal_item_in_dir` ("Abrir na pasta" no Nautilus) |

### Frontend
| Pacote | Uso |
|---|---|
| `svelte@5`, `vite`, `typescript` | UI |
| `@tauri-apps/api` v2 | `invoke` e `Channel` |
| `@tauri-apps/plugin-opener` | Revelar o arquivo no gerenciador de arquivos |

Ícones são SVG inline, sem fonte de ícones.

### Sistema (Ubuntu 24.04)
```bash
sudo apt install build-essential curl wget file libwebkit2gtk-4.1-dev libxdo-dev \
  libssl-dev libayatana-appindicator3-dev librsvg2-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
Node 22 já está instalado (via nvm).
