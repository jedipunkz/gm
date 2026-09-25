import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// The finder's palettes are read from the Rust source at build time, so the
// theme picker on the site can never drift from what `gm` actually ships.
// The build runs from site/, one level below the repository root.
const SOURCE = resolve(process.cwd(), "../src/finder/theme.rs");

export interface Theme {
  name: string;
  BgHi: string;
  Border: string;
  Comment: string;
  Fg: string;
  Blue: string;
  Cyan: string;
  Magenta: string;
  Green: string;
  Yellow: string;
  Orange: string;
  Red: string;
  Light: boolean;
  // Not part of theme.rs: gm paints on the terminal's own background. These
  // are the backgrounds each palette's terminal scheme is designed for.
  Bg: string;
}

const BACKGROUNDS: Record<string, string> = {
  tokyonight: "#1a1b26",
  "solarized-dark": "#002b36",
  "solarized-light": "#fdf6e3",
  "kanagawa-wave": "#1f1f28",
  "catppuccin-latte": "#eff1f5",
  "catppuccin-frappe": "#303446",
  "catppuccin-macchiato": "#24273a",
  "catppuccin-mocha": "#1e1e2e",
  "rose-pine": "#191724",
  dracula: "#282a36",
};

// Each colour as the site names it, and the field theme.rs keeps it in.
const COLOR_KEYS = [
  ["BgHi", "bg_hi"], ["Border", "border"], ["Comment", "comment"], ["Fg", "fg"],
  ["Blue", "blue"], ["Cyan", "cyan"], ["Magenta", "magenta"], ["Green", "green"],
  ["Yellow", "yellow"], ["Orange", "orange"], ["Red", "red"],
] as const;

export const DEFAULT_THEME = "tokyonight";

export function loadThemes(): Theme[] {
  const src = readFileSync(SOURCE, "utf8");
  const block = src.match(/pub const THEMES: &\[\(&str, Theme\)\] = &\[([\s\S]*?)\n\];/);
  if (!block) throw new Error(`no THEMES table in ${SOURCE}`);

  const themes: Theme[] = [];
  for (const m of block[1].matchAll(/\("([\w-]+)",\s*Theme\s*\{([\s\S]*?)\}\)/g)) {
    const [, name, body] = m;
    const t: Record<string, string | boolean> = { name, Light: /\blight:\s*LIGHT\b/.test(body) };
    for (const [key, field] of COLOR_KEYS) {
      const c = body.match(new RegExp(`\\b${field}:\\s*"(#[0-9a-fA-F]{6})"`));
      if (!c) throw new Error(`theme ${name}: missing ${field} in ${SOURCE}`);
      t[key] = c[1];
    }
    t.Bg = BACKGROUNDS[name] ?? (t.Light ? "#fafafa" : "#16161e");
    themes.push(t as unknown as Theme);
  }
  if (themes.length === 0) throw new Error(`no themes parsed from ${SOURCE}`);
  // Same order as `gm` prints them: the default first, the rest sorted.
  return themes.sort((a, b) =>
    a.name === DEFAULT_THEME ? -1 : b.name === DEFAULT_THEME ? 1 : a.name.localeCompare(b.name),
  );
}

// blend mirrors theme.rs: mixes two #rrggbb colours, t running from a to b.
export function blend(a: string, b: string, t: number): string {
  const p = (h: string) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
  const [x, y] = [p(a), p(b)];
  return "#" + x.map((v, i) => Math.round(v + (y[i] - v) * t).toString(16).padStart(2, "0")).join("");
}

// The CSS variables the terminal mock paints with, derived the way
// Theme.Styles() derives the finder's styles.
export function themeVars(t: Theme): Record<string, string> {
  return {
    "--t-bg": t.Bg,
    "--t-bghi": t.BgHi,
    "--t-border": t.Border,
    "--t-comment": t.Comment,
    "--t-fg": t.Fg,
    "--t-blue": t.Blue,
    "--t-cyan": t.Cyan,
    "--t-magenta": t.Magenta,
    "--t-green": t.Green,
    "--t-yellow": t.Yellow,
    "--t-orange": t.Orange,
    "--t-red": t.Red,
    "--t-row": blend(t.Comment, t.Fg, 0.35),
    "--t-commit": blend(t.Blue, t.Comment, 0.35),
    "--t-ref-remote": blend(t.Blue, t.Comment, 0.5),
    "--t-ref-tag": blend(t.Cyan, t.Comment, 0.35),
  };
}
