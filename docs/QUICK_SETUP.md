# QUICK_SETUP — Do zero ao app rodando

Guia para Ubuntu 24.04. Os outros documentos: [PLAN](PLAN.md) (etapas), [ARCHITECTURE](ARCHITECTURE.md) (como funciona) e [DECISIONS](DECISIONS.md) (por quê).

## 1. Pré-requisitos (uma vez só)

```bash
# Dependências de sistema do Tauri (WebKitGTK etc.)
sudo apt install build-essential curl wget file libwebkit2gtk-4.1-dev libxdo-dev \
  libssl-dev libayatana-appindicator3-dev librsvg2-dev

# Rust (toolchain stable)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Node 22+ (aqui instalado via nvm)
node --version
```

**Opcional, para o card do Docker:** o usuário precisa estar no grupo `docker`. Sem isso, o app funciona normalmente, mas o espaço do Docker aparece como "sem acesso".

```bash
id -nG | grep -w docker || sudo usermod -aG docker "$USER"   # depois, sair e entrar na sessão
```

## 2. Instalar as dependências do projeto

```bash
cd ~/storage_LKR
npm install          # frontend + CLI do Tauri
cargo fetch          # crates Rust (opcional: o primeiro build baixa sozinho)
```

## 3. Rodar o app em desenvolvimento

```bash
npm run tauri dev
```

- O primeiro build leva alguns minutos. Os seguintes levam segundos.
- Alterações no frontend recarregam na hora. Alterações em `src-tauri/` ou `crates/scanner/` recompilam e reabrem a janela.
- **DevTools:** clique com o botão direito na janela e escolha **Inspecionar**. Os eventos do scan aparecem no Console.
- **Para parar o app/servidor:** pressione `Ctrl + C` no terminal onde o comando está rodando.
- **Se o processo travar ou ficar rodando em segundo plano:**
  ```bash
  pkill -f "tauri dev"
  pkill -f "vite"
  ```

## 4. Testes e checagens

```bash
cargo test --workspace                   # motor + ponte Tauri
cargo clippy --workspace --all-targets   # lint Rust
cargo fmt --all                          # formatação Rust
npm run check                            # tipos TypeScript/Svelte
```

## 5. CLI do motor (benchmark e diagnóstico)

O motor roda sem a janela. É útil para conferir números contra `df` e `du`.

```bash
cargo build --release -p scanner --bin scan

target/release/scan                  # escaneia / (Docker incluso, ~7 s no total)
target/release/scan / --no-docker    # só o walk (~2,2 s com cache quente)
target/release/scan ~ --top 5        # $HOME, com os 5 maiores por categoria
target/release/scan / --threads 4    # muda o número de threads (padrão: 2× núcleos)
```

Conferência rápida: a soma do escaneado com o Docker e o "reservado/sem acesso" deve bater com o `df -B1 /`. E `scan ~` deve bater com `du -sxB1 ~`.

## 6. Gerar o pacote `.deb`

```bash
npm run tauri build
sudo apt install ./target/release/bundle/deb/*.deb
```

- O pacote se chama `storage-looker` e instala o binário `/usr/bin/storage-looker` e o atalho "Storage Looker" no menu do GNOME (categoria Utilitários).
- O bundle fica na pasta `target/` da raiz, porque o projeto é um workspace Cargo.
- Para trocar o ícone: `npx tauri icon caminho/para/logo.png` (PNG quadrado com fundo transparente) regenera `src-tauri/icons/`.
- Para desinstalar: `sudo apt remove storage-looker`. O cache do último scan fica em `~/.cache/storage-lkr/` e pode ser apagado à mão.

## 7. Onde está cada coisa

| Caminho | O que é |
|---|---|
| `crates/scanner/` | Motor: walk, classificação, Docker, `statvfs`. Sem Tauri |
| `crates/scanner/src/bin/scan.rs` | CLI de benchmark |
| `src-tauri/src/state.rs` | Ciclo do scan e eventos (`ScanEvent`) |
| `src-tauri/src/state/cleanup.rs` | Ações de limpeza e a validação delas |
| `src-tauri/src/state/cache.rs` | Cache do último scan (`~/.cache/storage-lkr/last.json`) |
| `src-tauri/src/commands.rs` | Commands chamados pelo frontend |
| `src-tauri/build.rs` + `capabilities/default.json` | Quais commands a janela pode chamar |
| `src/lib/api.ts` | Funções e tipos TypeScript dos commands |
| `src/routes/+page.svelte` | Tela do app |
| `docs/` | Plano, arquitetura, decisões e este guia |

**Para adicionar um command novo**, mexa em quatro lugares:
1. a função em `commands.rs`;
2. o registro em `lib.rs` (`generate_handler!`);
3. o nome em `build.rs` (`COMMANDS`);
4. a permissão `allow-<nome-com-hífens>` em `capabilities/default.json`.

Se faltar um dos dois últimos, a chamada é recusada em tempo de execução.

## 8. Problemas comuns

| Sintoma | Causa provável e solução |
|---|---|
| `cargo: command not found` | Rode `source ~/.cargo/env` ou abra um terminal novo |
| Build falha com `webkit2gtk-4.1` / `javascriptcoregtk` | Faltam as libs `-dev` do passo 1 |
| Command recusado no Console (`not allowed`) | Command fora do `build.rs` ou da capability (veja a seção 7) |
| Card Docker não aparece / "sem acesso" grande | Usuário fora do grupo `docker`, ou daemon parado (`systemctl status docker`) |
| Primeiro scan bem mais lento (~12 s) | Cache de disco frio; os próximos levam ~2 s |
| Porta 1420 ocupada no `tauri dev` | Instância anterior aberta em background: `pkill -f "tauri dev"` ou `pkill -f "vite"` |
