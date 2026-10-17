/// Commands do app. Declará-los aqui faz o Tauri exigir permissão explícita para cada um
/// (`allow-<command>` em `capabilities/default.json`), em vez de liberar todos por padrão.
const COMMANDS: &[&str] = &[
    "disk_info",
    "last_scan",
    "start_scan",
    "cancel_scan",
    "category_files",
    "reveal",
    "move_to_trash",
    "empty_trash",
    "docker_prune",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("falha ao gerar o contexto do Tauri");
}
