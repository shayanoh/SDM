import type { GrabberPackage, GrabberRow, GrabberView } from "../bindings";
import { rowsByPackage } from "./grabberRows";
import { GRABBER_HEADER_HEIGHT, GRABBER_ROW_HEIGHT } from "./layout";

/** One line of the grabber list: a package header or a row. */
export type FlatLine =
  | {
      kind: "header";
      key: string;
      height: number;
      /** The package's position among the packages (zebra striping). */
      index: number;
      pkg: GrabberPackage;
    }
  | {
      kind: "row";
      key: string;
      height: number;
      /** The row's position within its package, or among the loose rows (zebra striping). */
      index: number;
      row: GrabberRow;
    };

/** Rows in no package first, then each package's header followed by its rows. */
export function flatLines(view: GrabberView): FlatLine[] {
  const { loose, packages } = rowsByPackage(view);
  const lines: FlatLine[] = loose.map((row, index) => ({
    kind: "row",
    key: row.id,
    height: GRABBER_ROW_HEIGHT,
    index,
    row,
  }));
  packages.forEach(({ pkg, rows }, packageIndex) => {
    lines.push({
      kind: "header",
      key: `package:${pkg.id}`,
      height: GRABBER_HEADER_HEIGHT,
      index: packageIndex,
      pkg,
    });
    rows.forEach((row, index) =>
      lines.push({
        kind: "row",
        key: row.id,
        height: GRABBER_ROW_HEIGHT,
        index,
        row,
      }),
    );
  });
  return lines;
}

/** The ids of the selectable rows, top to bottom (headers are not selectable). */
export const selectableIds = (lines: FlatLine[]): string[] =>
  lines.flatMap((line) => (line.kind === "row" ? [line.row.id] : []));
