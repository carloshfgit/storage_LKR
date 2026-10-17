# PLAN — Etapas de desenvolvimento

Cada etapa termina com algo **executável e verificável**. Detalhes técnicos estão em [ARCHITECTURE.md](ARCHITECTURE.md), e a justificativa da stack em [STACK_CHOICE.md](STACK_CHOICE.md).

**Escopo do MVP:**
- Barra com 7 segmentos: Fotos, Vídeos, Áudio, Documentos, Apps/Sistema, Outros e Livre.
- Detalhe por categoria.
- Abrir na pasta, mover para a Lixeira e esvaziar a Lixeira com confirmação.

---

## Etapa 0 — Ambiente
- [x] Instalar Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- [x] Instalar as dependências do sistema:
  ```bash
  sudo apt install build-essential curl wget file libwebkit2gtk-4.1-dev libxdo-dev \
    libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```
- [x] Gerar o projeto: `npm create tauri-app@latest` (Svelte + TypeScript), com saída na raiz do repositório.
- [x] Converter para workspace Cargo (`[workspace] members = ["crates/scanner", "src-tauri"]`) e criar o `crates/scanner` vazio.
- [x] `git init` + `.gitignore` (`target/`, `node_modules/`, `dist/`).

**Pronto quando:** `npm run tauri dev` abre a janela padrão.

---

## Etapa 1 — Motor de scan (CLI)
- [ ] `model.rs`: `Category`, `FileEntry`, `CategoryStats`, `ScanResult`.
- [ ] `disk.rs`: `disk_info("/")` via `rustix::fs::statvfs`.
- [ ] `walk.rs`:
  - jwalk paralelo, sem cruzar filesystem (`st_dev`), sem seguir symlinks
  - `st_blocks * 512` e dedupe de hardlinks
  - top-N com heap
  - progresso com rate-limit e cancelamento
- [ ] Na Etapa 1, `classify` é provisório e só olha a extensão.
- [ ] `bin/scan.rs`: imprime a tabela por categoria, o restante, os diretórios negados e o tempo.

**Pronto quando:**
- `cargo run --release -p scanner --bin scan` termina e total + restante bate com o `df /` (±1%).
- Scan apontado para `$HOME` ≈ `du -sx ~` (±1%).
- `/` com cache quente leva **menos de 5 s**.

---

## Etapa 2 — Classificação definitiva
- [ ] Implementar as 6 regras ordenadas e a tabela de extensões de ARCHITECTURE §5.
- [ ] Testes unitários (pelo menos estes casos):

| Caminho | Esperado |
|---|---|
| `/usr/share/icons/x.png` | Apps/Sistema |
| `~/.cache/mozilla/a.jpg` | Outros |
| `~/.local/share/Trash/files/v.mp4` | Outros (conta em `trash_bytes`) |
| `~/Vídeos/Férias.MKV` | Vídeos |
| `~/snap/spotify/foo.mp3` | Apps/Sistema |
| `~/Downloads/ubuntu.iso` | Outros |
| `~/Documentos/cv.pdf` | Documentos |

**Pronto quando:** `cargo test -p scanner` passa, e o CLI mostra uma distribuição plausível na máquina real.

---

## Etapa 3 — Ponte Tauri
- [ ] `state.rs` (`AppState`) e `commands.rs`: `disk_info`, `start_scan` (Channel), `cancel_scan`, `category_files`.
- [ ] `capabilities/default.json` expondo só esses commands.
- [ ] `api.ts` com os tipos espelhando `ScanEvent`/`ScanResult`.

**Pronto quando:** o DevTools da janela mostra eventos `progress` chegando e um `done` final, e `cancel_scan` interrompe o scan.

---

## Etapa 4 — Tela principal
- [ ] `theme.css`: tokens de cor, claro/escuro e tipografia.
- [ ] `format.ts`: bytes decimais em pt-BR.
- [ ] Header com nome do disco e "X GB de Y GB usados".
- [ ] `StorageBar`: 7 segmentos com `tweened`, que crescem ao vivo durante o scan, com "Livre" desenhado logo após `disk_info`.
- [ ] `Legend` ordenada por tamanho e botão "Reescanear".
- [ ] Estados: escaneando (com contador de arquivos), pronto e erro.

**Pronto quando:** a barra anima durante o scan, soma exatamente 100% no fim e alterna corretamente entre tema claro e escuro.

---

## Etapa 5 — Detalhe da categoria
- [ ] `CategoryDetail` + `FileRow`: top-N ordenado por tamanho, com alternância para ordenar por data.
- [ ] "Abrir na pasta" via `reveal` (`tauri-plugin-opener`).
- [ ] Cards especiais: "Reservado / sem acesso" (Apps/Sistema) e "Lixeira" (Outros).

**Pronto quando:** clicar no segmento "Vídeos" lista os maiores vídeos, e "Abrir na pasta" abre o Nautilus com o arquivo selecionado.

---

## Etapa 6 — Ações de limpeza
- [ ] `move_to_trash` com validação no Rust (canônico, sob `$HOME`, fora de apps, presente no último scan), usando o crate `trash`.
- [ ] Seleção múltipla, rodapé com o total e atualização da barra: bytes saem da categoria e entram em "Lixeira".
- [ ] `empty_trash` com diálogo de confirmação ("Apagar permanentemente X GB?"), seguido de nova leitura de `disk_info`.
- [ ] Teste manual: tentar mandar para a lixeira um caminho fora de `$HOME` via DevTools precisa ser rejeitado.

**Pronto quando:**
- O arquivo aparece na Lixeira do Ubuntu e pode ser restaurado pelo Nautilus.
- Esvaziar a Lixeira aumenta o "Livre" na mesma quantidade vista no `df`.

---

## Etapa 7 — Polimento e pacote
- [ ] Cache do último scan em `~/.cache/storage-lkr/last.json`: abre instantâneo e reescaneia em background.
- [ ] Ícone do app, título da janela e tamanho mínimo.
- [ ] Animações finas: entrada do detalhe e hover nos segmentos com tooltip.
- [ ] `npm run tauri build` → `.deb` → `sudo apt install ./src-tauri/target/release/bundle/deb/*.deb`.

**Pronto quando:** o app aparece no menu do GNOME, abre em menos de 1 s mostrando o último scan e atualiza sozinho.

---

## Backlog (pós-MVP)
1. Detalhar Apps por Snap, Flatpak e APT, com tamanho por app.
2. Cards de sugestão: "Downloads antigos", "Caches", "10 maiores arquivos".
3. Atualização incremental (inotify ou fanotify).
4. Modo privilegiado via `pkexec` para escanear o que hoje é "sem acesso".
5. Detecção por magic bytes para arquivos sem extensão.
6. Suporte a múltiplos discos e partições (seletor no header).
