// Avisos das telas de detalhe: o resultado de uma ação e, se houver, os itens recusados.
import type { PurgeReport, Rejected } from "./api";
import { formatBytes, formatCount } from "./format";

/** Uma linha de recusado, com os comandos que resolvem pelo terminal (se houver). */
export interface NoticeDetail {
  text: string;
  commands?: string[];
}

export interface Notice {
  kind: "ok" | "error";
  text: string;
  details?: NoticeDetail[];
}

const plural = (n: number, one: string, many: string) => `${formatCount(n)} ${n === 1 ? one : many}`;

/** Nome do item recusado: o caminho original ou, se nem estava na Lixeira, o id. */
const itemName = (path: string) => path.slice(path.lastIndexOf("/") + 1).replace(/\.trashinfo$/, "");

export function rejectedDetails(rejected: Rejected[]): NoticeDetail[] {
  return rejected.map((r) => ({ text: `${itemName(r.path)}: ${r.reason}`, commands: r.commands }));
}

/** Resultado de apagar da Lixeira (os selecionados ou tudo). */
export function purgeNotice(report: PurgeReport, emptied: boolean): Notice {
  const parts: string[] = [];
  if (report.purged > 0) {
    parts.push(
      `${plural(report.purged, "item apagado", "itens apagados")}: ${formatBytes(report.freed_bytes)} liberados no disco.`,
    );
  } else if (emptied && report.rejected.length === 0) {
    parts.push("A Lixeira já estava vazia.");
  }
  if (report.rejected.length > 0) {
    parts.push(
      `${plural(report.rejected.length, "item não pôde ser apagado", "itens não puderam ser apagados")}:`,
    );
  }
  return {
    kind: report.rejected.length > 0 ? "error" : "ok",
    text: parts.join(" "),
    details: rejectedDetails(report.rejected),
  };
}
