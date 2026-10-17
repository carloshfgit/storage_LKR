// Nome e cor de cada categoria na interface. Os nomes espelham `Category::label` do Rust.
import type { Category } from "./api";

export const CATEGORY_LABELS: Record<Category, string> = {
  photos: "Fotos",
  videos: "Vídeos",
  audio: "Áudio",
  documents: "Documentos",
  apps_system: "Apps/Sistema",
  other: "Outros",
};

export const CATEGORY_COLORS: Record<Category, string> = {
  photos: "var(--cat-photos)",
  videos: "var(--cat-videos)",
  audio: "var(--cat-audio)",
  documents: "var(--cat-documents)",
  apps_system: "var(--cat-apps)",
  other: "var(--cat-other)",
};

/** Uma categoria como aparece na barra e na legenda. */
export interface Segment {
  key: Category;
  label: string;
  color: string;
  bytes: number;
  /** Linha extra na legenda, como a composição de Apps/Sistema. */
  note?: string;
}
