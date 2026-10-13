# UX review (2026-10-13)

Review of the first editor iteration, aimed at non-technical users maintaining
their own genealogy.

## Critical

1. **Silent, fragile save model.** Text edits commit only on focus loss: Ctrl+S, Undo
   or closing while a field is focused can lose the edit. Enter doesn't commit, Esc
   doesn't revert, Ctrl+Z is blocked while any text field has focus. The only dirty
   signal is a tiny "•".
   *Fix:* commit on Enter/blur, `flush_form()` before save/undo/close, a clear
   "Unsaved changes / All changes saved" indicator, timed toasts.
2. **Invisible errors.** Open/save/edit failures go to a faint status line; an
   invalid date is silently not committed and lost when switching person.
   *Fix:* error banner/modal, red outline + message on invalid fields, keep invalid input.
3. **Dead-end first run.** Only "Open a .familyx file…". No "New family tree", no
   recent files, no "Add the first person" empty state.

## High

4. **Core actions are tiny glyphs.** Add parent/spouse/child are 16px "+" buttons; unlink
   is "×", which reads as delete. *Fix:* labelled buttons ("+ Add child"), "Add child with
   ⟨spouse⟩" per spouse, "Remove relationship…" with a confirmation naming both people.
5. **Ambiguous tree clicks.** Single click selects, double click re-centres, two different
   outlines. *Fix:* single click = select + centre, Back/Forward history, one highlight,
   header "Viewing family of X", pointer cursor.
6. **Tree readability.** 2^n ancestor slots leave gaps; fit zoom makes text tiny; no hint
   that more generations exist; 11px grey help text. *Fix:* compact layout, "▲ more"
   badges, +/- zoom buttons, larger help.
7. **Expert-only date entry.** Three tiny numeric boxes; no "12 Mar 1950", no
   before/after/between, 31 Feb accepted. *Fix:* one free-text date field with a parsed preview.
8. **Terminology.** "Fact" → "Event", "Centre tree here" → "Show this person's tree",
   "×/unlink" → "Remove relationship", "about" → "approximately", "Details" → "Notes".
   "New person" should say it's not linked yet.
9. **Destructive actions.** "Delete person…" sits next to everyday buttons; move to the
   bottom, red, Cancel as default. Fact delete has no confirm; add "Undo" toasts with
   descriptions ("Undo: delete person").

## Medium

10. **Right panel hierarchy.** A 240px photo pushes fields below the fold; Family is at the
    bottom; couple facts are read-only grey text. *Fix:* 96px photo header with name and
    life span; sections Details / Events / Family.
11. **People list.** "(unnamed)" entries sort first; no sort/filter, no count, search only
    on names; double-click-to-centre is undiscoverable.
12. **Top bar.** Cramped flat buttons; add a File menu (Open recent, Save, Save as, Quit).
13. **Undo** is invisible while typing; label it ("Undo: rename").

## Low / accessibility

14. **Contrast.** Placeholders, status, disabled buttons are marginal; gender is colour-only.
    The tree canvas has no AccessKit nodes, so screen readers can't read it.
15. **Targets.** Controls are about 18–22px; raise `interact_size` to 28–32px, base font
    15–16px, Ctrl +/- zoom. Tree keyboard navigation (arrows).
16. Dark theme only; no photo editing; backups are mentioned only in the status line.

## Top 5 first

1. Reliable commit + visible save state + error toasts (1, 2)
2. Welcome screen: New / Open / recent; "Add first person" (3)
3. Labelled, larger relationship buttons with confirmations and undo toasts (4, 9)
4. Simpler tree interaction: click = select + centre, Back, one highlight, zoom/Fit buttons, compact layout (5, 6)
5. Free-text date field + plainer vocabulary + larger sizes (7, 8, 15)
