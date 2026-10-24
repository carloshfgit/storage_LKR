// Rótulos da aba Recursos usados por mais de um card.
import type { DiskKind, GpuSpec, GpuVendor } from "./api";

export const DISK_KIND_LABELS: Record<DiskKind, string> = {
  nvme: "NVMe",
  sata: "SATA",
  usb: "USB",
  mmc: "eMMC/SD",
  virtual: "Virtual",
  other: "Disco",
};

/** "NVMe · SSD", "SATA · HD", "eMMC/SD". */
export function diskKind(kind: DiskKind, rotational: boolean | null): string {
  const media = rotational === true ? " · HD" : rotational === false && kind !== "mmc" ? " · SSD" : "";
  return DISK_KIND_LABELS[kind] + media;
}

export const GPU_VENDOR_LABELS: Record<GpuVendor, string> = {
  amd: "AMD",
  nvidia: "NVIDIA",
  intel: "Intel",
  other: "",
};

/** O nome do `pci.ids` ou, sem ele, "GPU AMD (amdgpu)". */
export function gpuName(g: GpuSpec): string {
  if (g.model) return g.model;
  const vendor = GPU_VENDOR_LABELS[g.vendor];
  const base = vendor ? `GPU ${vendor}` : "GPU";
  return g.driver ? `${base} (${g.driver})` : base;
}

/** "integrada", "dedicada" ou nada (GPU virtual). */
export function gpuKind(g: GpuSpec): string {
  return g.integrated === null ? "" : g.integrated ? "integrada" : "dedicada";
}
