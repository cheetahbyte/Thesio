# Document themes (CSS)

Each Thesio document theme is a CSS file. Selecting a theme selects its CSS file. In Settings › Appearance, select **Edit CSS** to open it in your editor. Save your changes to redraw documents in every window immediately.

On first launch, the default theme is Feishu. Existing theme selections are preserved. The menu lists Feishu, Craft, Notion, and Obsidian in that order. Selecting a theme updates the page preview.

- Built-in themes (Feishu, Craft, Notion, and Obsidian) are in `assets/themes/`. Each file contains the current value of every property and can serve as a reference.
- When you edit a built-in theme, the app copies it to `themes/<id>.css` in the document library. This copy replaces the built-in version. The menu shows it as modified; restoring the default deletes the copy.
- Duplicating a theme creates `themes/<id>-copy.css`. Rename it to make it your own. You can also put another author's `.css` file in `themes/`; the app loads it automatically.
- Invalid values and unrecognized selectors or properties don't invalidate the theme. The app skips them and lists them in settings as not applied.

## File structure

```css
:root {
  --theme-name: "My theme";          /* Display name, required */
  --theme-name-en: "My theme";       /* Name in the English interface */
  --theme-author: "Your name";
  --theme-description: "Theme description"; /* Metadata, not shown in the menu */
  --theme-base: craft;               /* Block rendering: craft or feishu */

  --accent: hsl(254, 80%, 68%);      /* Custom variable, referenced with var() */
  --color-red: #d44c47;              /* Named color */
}

.page { color: #37352f; font-size: 16px; line-height: 1.5; max-width: 708px; }
h1 { font-size: 1.875em; font-weight: 600; margin-top: 32px; }
blockquote { border-left: 3px solid var(--accent); }
```

- Supports `/* comments */`, `var(--x, fallback)`, and comma-separated selectors. At-rules such as `@media` are ignored in full.
- Lengths: `px` (= pt), `em` (relative to body text for font sizes, otherwise relative to the current element's font size), `rem` (16), and unitless `line-height` (a multiplier).
- Colors: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()`, `hsl()`, `hsla()`, `transparent`, `white`, and `black`. Use `inherit` on a color property to use the text color.
- `font-family` uses the first installed font in the list. `system-ui`, `-apple-system`, and `sans-serif` use the system font; `ui-monospace` and `monospace` use the system monospace font.

## Selectors and properties

Themes with `--theme-base: feishu` render blocks in the Feishu style and use only properties marked in the Feishu column. Other properties are listed as not applied.

| Selector | Properties | Description | Feishu |
|---|---|---|---|
| `.page` (`body`) | `background-color` `color` `font-family` `font-size` `line-height` `caret-color` | Page and body text | ✓ |
| `.page` | `max-width` | Body column width for the standard page width; wider and full widths are unaffected | ✓ |
| `::selection` | `background-color` | Selected text background | ✓ |
| `p` | `margin-bottom` | Space between blocks | ✓ |
| `.title` | `font-family` `font-size` `line-height` `font-weight` `color` | Document title | ✓ |
| `h1`–`h9` | `font-family` `font-size` `line-height` `font-weight` `color` `margin-top` `margin-bottom` | Heading levels | Font size, line height, font weight, and vertical margins |
| `strong` (`b`) | `font-weight` `color` | Bold text | |
| `a` | `color` `text-decoration` (`underline` / `none`) `text-decoration-color` | Links; `underline` always draws an underline | `color` only |
| `a:hover` | `color` | Hovered link | ✓ |
| `code` | `color` `background-color` `border-color` `border-radius` `padding-left` | Inline code | |
| `pre` | `font-family` `font-size` `line-height` `color` `background-color` `border-color` `border-radius` | Code block | Colors and fonts |
| `.line-numbers` | `display` (`block` / `none`) `color` `background-color` | Code block line numbers | Colors |
| `.token.comment` `.token.keyword` `.token.tag` `.token.string` `.token.number` `.token.boolean` `.token.class-name` `.token.attr-name` `.token.function` | `color` | Code highlighting (uses Prism class names) | ✓ |
| `blockquote` | `border-left` (or `border-left-width` / `-color`) `padding-left` `color` `font-style` | Quote: vertical line, indentation, and text | Vertical line color only |
| `ul` | `padding-left` | Nested indentation | |
| `ul li::marker` / `ol li::marker` / `li::marker` | `color` | Bullet or number color | |
| `.task` (`input[type=checkbox]`) | `width` (= height) `border-radius` `border-color` | Task checkbox | `border-color` only |
| `.task:checked` | `background-color` `color` | Checked background and checkmark color | |
| `.task-done` | `color` `text-decoration` (`line-through` / `none`) | Completed task text | `color` only |
| `hr` | `border-top` (or `border-top-style` `-width` `-color`) | Divider: `dotted` / `solid` / `dashed` | Color only |
| `table` (`td`) | `font-size` `line-height` `border-color` `border-radius` | Table text and borders | `border-color` only |
| `th` | `background-color` | Table header background | ✓ |
| `.callout` | `border-radius` `border-width` `padding` (top / left) | Callout | |

### Named colors in `:root`

Documents store text, background, and callout colors by name (red, orange, yellow, and so on). The theme determines their displayed colors:

| Variable | Use |
|---|---|
| `--color-gray` … `--color-purple` | Text colors (gray, red, orange, yellow, green, blue, purple) |
| `--highlight-light-red`, `--highlight-red` … | Text highlighting (lighter with `light-*`, darker without a prefix; gray also has `medium-gray`) |
| `--background-light-red`, `--background-medium-red` … | Block backgrounds |
| `--callout-light-red`, `--callout-medium-red` … | Callout backgrounds |
| `--border-red` … | Callout borders |

### Other details: `--token-*`

Other document interface details have colors, including hovered and selected blocks, the table toolbar, and size selection cells. Override them in `:root` with `--token-<name>`. For the complete list and current values, see the other interface details section at the end of each built-in theme file.

## Scope

- Themes change only the document itself. They don't affect the sidebar, menus, settings, or other interface elements.
- Dark mode isn't available. You can set a dark page color, but the surrounding interface remains light.
- CSS doesn't control the overall structure of blocks, such as code block toolbars or table row and column controls.
