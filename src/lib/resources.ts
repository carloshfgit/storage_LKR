// Rótulos da aba Recursos usados por mais de um card.
import type { BatteryStatus, DiskKind } from "./api";

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

export const BATTERY_STATUS_LABELS: Record<BatteryStatus, string> = {
  charging: "Carregando",
  discharging: "Na bateria",
  full: "Carregada",
  not_charging: "Na tomada, sem carregar",
  unknown: "Estado desconhecido",
};
