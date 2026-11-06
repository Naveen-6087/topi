/**
 * A small pixel alphabet for swapping single letters in headings, the way a packet looks once it is
 * encrypted. Each glyph is 5 columns by 9 rows: rows 0-1 ascend, rows 2-6 are the x-height, rows 7-8
 * descend, so a glyph sits on the same baseline as the typeface around it.
 */
const BLANK = ".....";
const x = (body: string[], top = [BLANK, BLANK], bottom = [BLANK, BLANK]) => [...top, ...body, ...bottom];

export const GLYPHS: Record<string, string[]> = {
  a: x(["####.", "....#", "#####", "#...#", "#####"]),
  c: x(["#####", "#....", "#....", "#....", "#####"]),
  d: x(["#####", "#...#", "#...#", "#...#", "#####"], ["....#", "....#"]),
  e: x(["#####", "#...#", "#####", "#....", "#####"]),
  g: x(["#####", "#...#", "#...#", "#...#", "#####"], [BLANK, BLANK], ["....#", "#####"]),
  h: x(["####.", "#...#", "#...#", "#...#", "#...#"], ["#....", "#...."]),
  i: x([".##..", "..#..", "..#..", "..#..", ".###."], ["..#..", BLANK]),
  l: x(["..#..", "..#..", "..#..", "..#..", ".###."], [".##..", "..#.."]),
  n: x(["####.", "#...#", "#...#", "#...#", "#...#"]),
  o: x(["#####", "#...#", "#...#", "#...#", "#####"]),
  p: x(["#####", "#...#", "#...#", "#...#", "#####"], [BLANK, BLANK], ["#....", "#...."]),
  r: x(["#.###", "##...", "#....", "#....", "#...."]),
  s: x(["#####", "#....", "#####", "....#", "#####"]),
  t: x(["####.", ".#...", ".#...", ".#...", ".###."], [".#...", ".#..."]),
  u: x(["#...#", "#...#", "#...#", "#...#", "#####"]),
  v: x(["#...#", "#...#", "#...#", ".#.#.", "..#.."]),
  y: x(["#...#", "#...#", "#...#", "#...#", "#####"], [BLANK, BLANK], ["....#", "#####"]),
};

/** The glyph as SVG rectangles in a 5 by 9 box. */
export function glyphRects(ch: string): string {
  const rows = GLYPHS[ch];
  if (!rows) return "";
  return rows
    .flatMap((row, y) => [...row].map((c, x) => (c === "#" ? `<rect x="${x}" y="${y}" width="1.02" height="1.02"/>` : "")))
    .join("");
}
