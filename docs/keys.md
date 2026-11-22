# Keys

Rain has three shortcut profiles: **Vim** (the default), **VS Code** and **JetBrains**. A profile
only decides the command keys. The universal keys below work the same in every profile, and no
profile or override can take them.

Pick a profile, and change any command's keys, in **Keyboard shortcuts**: the `⋯` menu, the
command palette (`Keyboard shortcuts`, or `Keyboard shortcuts: VS Code` to switch at once), F1 in
Vim, `Ctrl+,` in Vim and VS Code, `Ctrl+Alt+S` in JetBrains. Every hint in the window (tooltips,
the status line, the palette, the menus) shows the keys of the profile and overrides in force.

## Universal keys

| Keys | What they do |
|---|---|
| Up / Down | Move the diff cursor by line, keeping its column |
| Left / Right | Move by character; at a line's end they go on to the next or previous line |
| Ctrl+Left / Ctrl+Right | Move by word |
| Home / End | First non-blank character (again: column 0) / last character |
| Ctrl+Home / Ctrl+End | The file's first / last line |
| PageUp / PageDown | Most of a page; the cursor moves with it and keeps its place on screen |
| Shift + any of the above | Extend the selection (it is Vim's visual selection, so copy, comment and ask take it) |
| Ctrl+C | Copy the selection; with none, the cursor's line |
| Ctrl+A | Select the current file's lines |
| Escape | Clear the selection, then close the top layer (palette, overlay, composer, find, filter) |
| Enter | Open the focused file or item (lists, tree, palette, picker) |
| Mouse | Click puts the cursor; drag selects text; click and Shift+click on line numbers select lines |

The arrow keys go to whatever has focus first: the palette, the picker, the file tree and other
lists keep using them as before. The diff takes them only when it is the active surface (the
Changes tab, with no Map or Conversation over it).

In the Vim profile the Ctrl+P picker has two modes. It opens in insert mode: typing goes into the
query and the arrows move. Escape switches to normal mode, keeping the query: `j` / `k` and the
arrows move, `gg` / `G` go to the top and bottom, Enter goes, `i` or `/` type again, and Escape
closes. In the other profiles Escape closes the picker straight away.

## Command keys by profile

Bare letters are bound only in Vim; in VS Code and JetBrains every command key has a modifier or
is a function key, so nothing reads as typing. A blank cell means no default key: the command is
in the palette and on its button, and you can give it one.

| Command (`id`) | Vim | VS Code | JetBrains |
|---|---|---|---|
| Next line (`cursor-down`) | `j` | Down | Down |
| Previous line (`cursor-up`) | `k` | Up | Up |
| Next hunk (`next-hunk`) | `n` | Alt+F5 | F7 |
| Previous hunk (`prev-hunk`) | `p` | Alt+Shift+F5 | Shift+F7 |
| Next file (`next-file`) | `]` | Ctrl+PageDown | Alt+Right |
| Previous file (`prev-file`) | `[` | Ctrl+PageUp | Alt+Left |
| Next list section (`next-section`) | `J` | Ctrl+Shift+PageDown | Ctrl+Shift+PageDown |
| Previous list section (`prev-section`) | `K` | Ctrl+Shift+PageUp | Ctrl+Shift+PageUp |
| Back (`back`) | Alt+Left | Alt+Left | Ctrl+Alt+Left |
| Forward (`forward`) | Alt+Right | Alt+Right | Ctrl+Alt+Right |
| Next waiting on you (`next-waiting`) | `i` | F8 | F2 |
| Mark viewed and go to next (`viewed-and-next`) | Space | Ctrl+Enter | Ctrl+Enter |
| Toggle viewed (`toggle-viewed`) | Shift+Space | Ctrl+Shift+Enter | Ctrl+Shift+Enter |
| Accept this group (`accept-group`) | `A` | | |
| Undo the last delete or accept (`undo`) | `u` | Ctrl+Z | Ctrl+Z |
| Split / unified (`toggle-split`) | `s` | | |
| Stream / file at a time (`toggle-reader-mode`) | `S` | | |
| Show / hide the list (`toggle-list`) | Ctrl+B | Ctrl+B | Alt+1 |
| Zoom / restore (`zoom`) | Shift+Escape | Shift+Escape | Ctrl+Shift+F12 |
| Toggle whole file (`toggle-whole-file`) | `z` | | |
| Expand all context (`expand-all-context`) | `E` | | |
| Collapse all context (`collapse-all-context`) | `Z` | | |
| Collapse all files (`collapse-all-files`) | Ctrl+Shift+- | Ctrl+Shift+[ | Ctrl+Shift+- |
| Expand all files (`expand-all-files`) | Ctrl+Shift+= | Ctrl+Shift+] | Ctrl+Shift+= |
| Filter files (`filter-files`) | Ctrl+L | Ctrl+Shift+E | Ctrl+L |
| Clear file filter (`clear-filter`) | Ctrl+Shift+L | | |
| Go to file (`go-to-file`) | Ctrl+P | Ctrl+P | Ctrl+Shift+N |
| Command palette (`palette`) | Ctrl+K | Ctrl+Shift+P, F1 | Ctrl+Shift+A |
| Find (`find`) | `/` | Ctrl+F | Ctrl+F |
| Comment (`comment`) | `c` | Ctrl+/ | Ctrl+/ |
| Ask a question (`ask`) | `?` | Ctrl+I | Ctrl+I |
| Reply (`reply`) | `r` | | |
| Agree (`agree`) | `a` | | |
| Dismiss (`dismiss`) | `x` | | |
| Defer (`defer`) | `d` | | |
| Conversation (`conversation`) | `C` | Ctrl+Shift+C | Ctrl+Shift+C |
| Map (`map`) | `m` | | |
| Next / previous map view (`next-map-view`, `prev-map-view`) | `}` / `{` | | |
| Overview / Changes (`toggle-overview`) | `o` | Ctrl+Tab | Ctrl+Tab |
| Tour (`tour`) | `T` | | |
| Verify by hand: fold / unfold (`toggle-verify`) | `H` | | |
| Finish review (`finish-review`) | `F` | | Ctrl+K |
| Keyboard shortcuts (`keyboard-shortcuts`) | F1, Ctrl+, | Ctrl+, | Ctrl+Alt+S |
| Open in editor (`open-in-editor`) | Ctrl+O | Ctrl+O | F4 |
| Copy location (`copy-location`) | `Y` | Alt+Shift+C | Ctrl+Alt+Shift+C |
| Reload diff (`reload`) | `R` | F5 | Ctrl+Alt+Y |
| Toggle density (`toggle-density`) | Ctrl+Shift+D | | |
| Close / clear (`escape`) | Escape | Escape | Escape |
| Layout: auto, Go to Overview, Go to Changes, Copy all comments, Toggle theme | palette | palette | palette |

Where the IDE has the action, the key is the IDE's: VS Code's Go to File (Ctrl+P), Show All
Commands (Ctrl+Shift+P, F1), Find (Ctrl+F), Toggle Line Comment (Ctrl+/), inline chat (Ctrl+I),
Go to Next / Previous Difference (Alt+F5 / Alt+Shift+F5), Next / Previous Editor
(Ctrl+PageDown / Ctrl+PageUp), Go Back / Forward (Alt+Left / Alt+Right), Next Problem (F8),
Toggle Sidebar (Ctrl+B), Copy Path (Alt+Shift+C), Settings (Ctrl+,); IntelliJ's diff viewer
Next / Previous Difference (F7 / Shift+F7), Compare Next / Previous File (Alt+Right / Alt+Left),
Jump to Source (F4), Navigate Back / Forward (Ctrl+Alt+Left / Right), Find Action
(Ctrl+Shift+A), Go to File (Ctrl+Shift+N), Next Highlighted Error (F2), Project window (Alt+1),
Hide All Tool Windows (Ctrl+Shift+F12), Commit (Ctrl+K), Copy Reference (Ctrl+Alt+Shift+C),
Reload from Disk (Ctrl+Alt+Y), Settings (Ctrl+Alt+S), Collapse All / Expand All
(Ctrl+Shift+Minus / Ctrl+Shift+Equal).

VS Code's Fold All and Unfold All are two-chord sequences (Ctrl+K Ctrl+0, Ctrl+K Ctrl+J), and Rain
binds single chords only, so collapse and expand all files take Fold / Unfold's Ctrl+Shift+[ /
Ctrl+Shift+] instead. Vim's `zM` / `zR` would make `z` wait for a second key, so Vim uses the
JetBrains pair and keeps `z` for the whole file.

The tour's next and previous step follow Next hunk and Previous hunk; Space and Enter on the tour
card are the same in every profile.

## Vim, and its equivalent in every profile

Vim's motions and visual mode drive the diff only in the Vim profile. Each has a universal
equivalent:

| Vim | Every profile |
|---|---|
| `h` / `l` | Left / Right |
| `j` / `k` | Down / Up |
| `w` / `b` (`e`, `W`, `B`) | Ctrl+Right / Ctrl+Left |
| `0` / `^` / `$` | Home (twice for column 0) / Home / End |
| `gg` / `G` | Ctrl+Home / Ctrl+End |
| `Ctrl+D` / `Ctrl+U` | PageDown / PageUp (a whole page rather than half) |
| `v` | Shift + any cursor key, or a drag over the code |
| `V` | a press and drag on the line numbers (Shift+click extends); Ctrl+A for the whole file |
| `y` in visual, `yy` | Ctrl+C (the selection, else the line) |
| `c` / `?` in visual | the selection toolbar's Comment / Ask, or the profile's Comment / Ask keys |
| Escape in visual | Escape |
| `f{c}` `t{c}` `;` `,`, counts (`3w`, `5G`) | none needed: editors have no such keys |

## Overrides

Overrides live in the settings file (`%LOCALAPPDATA%\Rain\settings.json`), and the shortcuts
screen writes them for you:

```json
"keymap": {
  "profile": "vscode",
  "overrides": {
    "comment": ["Ctrl+M"],
    "ask": [],
    "go-to-file": ["Ctrl+P", "Ctrl+E"]
  }
}
```

- `profile` is `vim`, `vscode` or `jetbrains`; anything else reads as `vim`.
- An override replaces all of the command's keys; `[]` leaves it without one. A key an override
  gives a command is taken from the command the profile gave it to.
- Keys are written as the hints show them: `Ctrl+Shift+L`, `Alt+F5`, `J` (Shift+j), `?`,
  `Space`, `PageDown`, `F7`, `Ctrl+Shift+-`. `Cmd` reads as Ctrl. With Ctrl or Alt, Shift and a
  punctuation key is the character Shift types on a US layout (`Ctrl+Shift+-` is `Ctrl+_`).
- An entry Rain can't use is ignored and named in the status line: an unknown command, a key it
  can't read, a key two overrides both claim, or a universal key given to another command.

### Two keys in turn

A key can be two keys pressed one after the other, written with a space between them:
`"reload": ["g r"]`, `"toggle-density": ["Ctrl+Q Ctrl+D"]`. They work as in Vim:

- A key that starts no two-key shortcut acts at once, as it always did.
- A first key that does nothing by itself waits for the second, for as long as it takes.
- A first key that also does something by itself (`c` when `c a` is bound, or one of Vim's own
  motions such as `g` or `w`) waits `timeoutMs` (1000 when not set, Vim's `timeoutlen`) and then
  acts alone. The status line names such a key when the settings load.
- A second key that completes nothing lets the first act alone at once, and is then read as a
  key of its own. So with `g d` bound, `gg` still goes to the first line.
- Escape drops a waiting first key. So does moving into a text field.
- A universal key can't be either key of the two.

```json
"keymap": { "timeoutMs": 400, "overrides": { "reload": ["R", "g r"] } }
```

The shortcuts screen shows and removes two-key shortcuts, and records single keys.

In the shortcuts screen, press the keyboard button on a command's row and then the keys. A key
another command has asks first: **Replace** moves it, **Cancel** leaves both alone. The × on a
key removes it, the reset button puts the command back to the profile's keys, **Reset all** does
that for every command. **Save** writes the settings; **Cancel** or Escape throws the changes
away.
