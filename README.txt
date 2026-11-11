PLC
===

plc is a plain-text note-taking CLI. Every subcommand does one thing: it
creates or resolves a file and prints that file's path — nothing more. It
never opens an editor, never prompts, and is never interactive. A thin zsh
wrapper takes the printed path and opens it with $EDITOR (piping list output
through fzf where a picker is wanted). This keeps the binary pure and
scriptable: the file/path side is plc's job, the editor side is the shell's.

Notes are .md files under one vault ($PALACE_DIR), each seeded with a
stamped header and linked with [[wikilinks]]. One clock and one link graph
are shared across every note type — including ledgers.

    plc init       scaffold the vault directory tree
    plc daily      today's (or a given date's) daily note
    plc weekly     this ISO week's note
    plc shot       a timestamped snapshot note
    plc top        TOP.md — the vault landing page
    plc do         week-based "do" notes with a "last" pointer
    plc murmur     free-form named notes
    plc isg        enumerated writing notes (isg0, isg1, …)
    plc orphans    list notes with no links in or out
    plc stat       calendar heatmap + stats of daily-note activity
    plc ledger     plain-text double-entry ledger (alias: plc lr)
    plc doctor     check vault health and propose repairs
    plc config     show the resolved vault location ($PALACE_DIR)


-------------------------------------------------------------------------------
THE METHOD
-------------------------------------------------------------------------------

plc's conventions aren't arbitrary. The stamp header, the [[wikilink]] graph,
the four note domains, and the daily → weekly → do → seasons → longs
horizons all come from PALACE, a plain-text note-taking method that predates
the binary. Its notes are text you could keep by hand; plc only removes the
typing.

The method's own documents live in docs/, kept as they were written:

    docs/README.txt        the PALACE overview, man-page style
    docs/MANIFEST          motivation, the four domains, the principles
    docs/STATE-OF-THE-ART  rules, note structure, tags, Zettelkasten
    docs/FORMAT            formatting conventions + hierarchy of plans
    docs/NAVIGATION        the hierarchy of plans
    docs/PALACE.md         the vault's core landing page
    docs/START PAGE        the vault's start page

-------------------------------------------------------------------------------
SETUP
-------------------------------------------------------------------------------

Build and install the binary, point it at a vault, and scaffold it:

    > cargo install --path plc   # → ~/.cargo/bin/plc
    > export PALACE_DIR=~/vault  # the vault root (required by most commands)
    > plc init
    init: ~/vault — 9 created, 0 already present (9 dirs)

plc takes the vault path from $PALACE_DIR alone (a leading ~ is expanded),
then validates it in stages (its parent must exist, the dir must exist, and
it must contain notes/), with a pointed error at whichever stage fails.

Set PALACE_DIR as a literal in your shell rc. plc never writes the path
anywhere, so no process can redirect another's vault. Editors and scripts
that need the absolute path can ask plc for it:

    > plc config                    # prints the resolved vault path

Earlier versions fell back to a "PALACE_DIR = …" line in ~/.plcrc, written
by plc config --set. Both are gone: ~/.plcrc is no longer read, and when
PALACE_DIR is unset, the error points out a leftover ~/.plcrc.

plc doctor verifies that PALACE_DIR is set and resolves to a vault.

-------------------------------------------------------------------------------
ENVIRONMENT
-------------------------------------------------------------------------------

    PALACE_DIR    the vault root (required; the only source of the path)
    PLC_CURRENCY  default currency for plc ledger when a line omits one
                  (default: EUR)

-------------------------------------------------------------------------------
GLOBAL OPTIONS
-------------------------------------------------------------------------------

These apply to whichever subcommand runs:

    -x, --postfix TEXT   append +TEXT to the created note's filename, before
                         the extension (2026-07-14T20.28.md → …+TEXT.md)
    -t, --tag TAG        seed an extra [[TAG]] wikilink line into the new
                         note's body (header-seeded notes only)

-------------------------------------------------------------------------------
THE VAULT
-------------------------------------------------------------------------------

plc init creates the canonical tree (idempotent — existing dirs are left
alone):

    <PALACE_DIR>/
      .plc/                   # settings + state: config, pointers, logs
      TOP.md                  # plc top
      notes/
        archive/
        management/
          daily/YYYY/MM/…     # plc daily, plc shot, plc ledger
          do/                 # plc do
          weekly/             # plc weekly
        me/writing/isg/       # plc isg
        me/writing/murmur/    # plc murmur
        projects/
        sensible/
      templates/

Every note plc creates (when the file is absent or empty) is seeded with a
header: a stamp line and a [[tag]] wikilink, e.g.

    isg 2026-07-19 15:22:06 +0200

    [[daily]]

The stamp prefix is the author handle isg (except isg notes, which lead with
their own index). plc only creates/append; it never rewrites your body, so
you can edit freely by hand.

-------------------------------------------------------------------------------
PLC INIT
-------------------------------------------------------------------------------

Usage: plc init [DIR]

Scaffold the canonical vault directories under DIR (or $PALACE_DIR). Reports
how many were created vs. already present. Safe to re-run.

-------------------------------------------------------------------------------
PLC DAILY
-------------------------------------------------------------------------------

Usage: plc daily [DD [MM [YY|YYYY]]]

Create or resolve a daily note at
notes/management/daily/YYYY/MM/YYYY-MM-DD.md (tag [[daily]]). With no
arguments it's today; a date can be given positionally (day, then month,
then year) or via flags:

    -d, --day DD          -m, --month MM        -y, --year YY|YYYY
    --check               resolve only: print new|old<TAB><path> without
                          creating (lets the shell prompt before writing)

Any explicitly given date field back-dates the note (marked with a *).

    > plc daily                 # today
    > plc daily 1 8             # the 1st of August, this year
    > plc daily --check         # new<TAB>/…/2026-07-19.md (nothing written)

-------------------------------------------------------------------------------
PLC WEEKLY
-------------------------------------------------------------------------------

Create or resolve this ISO week's note at
notes/management/weekly/<GGGG-Www>.md (e.g. 2026-W29.md, tag [[weekly]]).

-------------------------------------------------------------------------------
PLC SHOT
-------------------------------------------------------------------------------

Usage: plc shot [-p PATH] [-i TEXT] [-H]

Create a timestamped snapshot note YYYY-MM-DDTHH.MM.md (tag [[shots]]), by
default in the current month's daily dir.

    -p, --path PATH    target dir (created if absent). A leading @ resolves
                       against the vault root (@notes/inbox); otherwise the
                       path is relative to the cwd (or absolute). Default:
                       the vault daily dir.
    -i, --inline TEXT  write TEXT as the body under the stamp (no [[shots]]
                       tag)
    -H, --no-header    omit the stamp header (with -i, writes just the text;
                       alone, an empty note)

-------------------------------------------------------------------------------
PLC TOP
-------------------------------------------------------------------------------

Create or resolve TOP.md at the vault root (tag [[top]]) — the palace
landing page.

-------------------------------------------------------------------------------
PLC DO
-------------------------------------------------------------------------------

Usage: plc do [-n | -l FILE | -L]

Week-based "do" notes (do-<GGGG-Www>.md under notes/management/do) with a
"last" pointer stored in <PALACE_DIR>/.plc/last-do.

    (no flag)          resolve the "last" do-note's path (errors if
                       unset/stale)
    -n, --new          create this ISO week's do-note and mark it "last"
    -l, --last FILE    re-point "last" at an existing do-note (basename)
    -L, --list         list do-notes, marking the "last" one with *

-------------------------------------------------------------------------------
PLC MURMUR
-------------------------------------------------------------------------------

Usage: plc murmur [NAME] | -n NAME | -l

Free-form named notes under notes/me/writing/murmur (.md appended if
missing).

    (positional NAME)  create/resolve NAME
    -n, --new NAME     same, as a flag
    -l, --list         list murmur notes newest-first (zsh pipes this to fzf)

-------------------------------------------------------------------------------
PLC ISG
-------------------------------------------------------------------------------

Usage: plc isg [NAME] | -l | --list | -c | --continue [INDEX]

Enumerated writing notes under notes/me/writing/isg.

    (no flag) / -l, --last   open the most recently modified isg note
    NAME                     open an existing note by basename (.md optional)
    --list                   list isg notes newest-first (fzf)
    -c, --create             create the next enumerated note: isg0, isg1, …
    --continue [INDEX]       continue note INDEX (or the latest) with the
                             next letter suffix: isg<INDEX>a, isg<INDEX>b, …

-------------------------------------------------------------------------------
PLC ORPHANS
-------------------------------------------------------------------------------

Usage: plc orphans [-r DIR] [-v]

List orphan notes — those with neither outbound nor inbound [[wikilinks]].
Accounts, categories, and tags used by plc ledger are links too, so they
take part in this graph.

    -r, --root DIR   search root (default: <PALACE_DIR>/notes)
    -v, --verbose    show mtime + size beside each path

-------------------------------------------------------------------------------
PLC STAT
-------------------------------------------------------------------------------

Usage: plc stat [DD MM YY] [--type month|year] [-m M] [-y Y]
                [--layout git|tab] [-p]

Render daily-note activity (by note byte-size) as a calendar heatmap, with
a stats block. A whole month or year is shown, so a positional day is
discarded.

    --type month|year     scope (default: month)
    -m, --month M         month 1-12 (default: current)
    -y, --year Y          year, 2- or 4-digit (default: current)
    --layout git|tab      year layout: GitHub-style grid or a month table
    -p, --plot            replace the heatmap with an ASCII line chart

-------------------------------------------------------------------------------
PLC LEDGER
-------------------------------------------------------------------------------

Plain-text, double-entry ledgers kept in the same vault — one +ledger.md
file per day. Quick tour:

    > plc ledger add 4.50 Blue Bottle -a cash -c coffee    # an expense
    > plc ledger add 2400 pay -a bnp -c salary --income    # income
    > plc ledger add 200 ATM -a bnp --to cash              # a transfer

    > plc ledger report       # net, by account/category
    > plc ledger bal          # net-worth snapshot
    > plc ledger reg coffee   # register, filtered

ledger has its own subcommands (add, edit, rm, report, reg, balance/bal,
check, fmt, stat, account/acc, last, undo) and a full grammar for dates,
transfers, splits, tags, balance assertions, hierarchy, and inline
arithmetic in the amount. You can declare a vocabulary of accounts
(--physical) and categories (--ephemeral) with plc ledger account -a NAME
that add validates against — and rename one across every ledger with
plc ledger account -r OLD NEW (ids preserved) — and reverse a mistake with
plc ledger undo. Settings live in .plc/config.

Every added transaction carries a stable ^-prefixed short hash (git-commit
style); plc ledger edit <id> targets one by that hash (a unique prefix, like
git) to change a field in place or print its path:line for your editor —
the frozen id survives the edit.

See docs/ledger.md for the complete ledger manual.

-------------------------------------------------------------------------------
PLC DOCTOR
-------------------------------------------------------------------------------

Usage: plc doctor [--fix]

Check the vault's health and propose repairs. It runs even without a
resolved vault, so it can diagnose the config itself: whether PALACE_DIR is
set and resolves to a real vault. It then audits the ledger .plc/config —
accounts/categories used but not declared (or vice versa), a name declared
as both @ and #, a missing default currency, transactions missing a stable
^id, a legacy .last-do pointer — printing a fix command for each. --fix
applies the safe ones (importing names, setting the currency, backfilling
ids, migrating the pointer). It's built to grow into a whole-vault checkup
(orphans, stale pointers, broken links).

    > plc doctor
    > plc doctor --fix

-------------------------------------------------------------------------------
PLC CONFIG
-------------------------------------------------------------------------------

Usage: plc config

Print the resolved vault location: $PALACE_DIR with a leading ~ expanded.
It fails, saying how to set it, when PALACE_DIR is unset or empty.

    > plc config

-------------------------------------------------------------------------------
HOW IT RUNS DAY TO DAY
-------------------------------------------------------------------------------

plc prints a path; your shell does the rest. The intended pattern (zsh):

    daily()  { ${EDITOR:-vim} "$(plc daily "$@")" }
    murmur() { plc murmur -l | fzf |
        xargs -r -I{} ${EDITOR:-vim} "$(plc murmur {})" }

For an interactive login you might need to run (e.g.) a decrypt/mount step
for the vault yourself before plc can see notes/ — plc will tell you which
validation stage failed if the vault isn't ready.

-------------------------------------------------------------------------------
PLC-LSP
-------------------------------------------------------------------------------

A language server for the vault's markdown, a separate binary that speaks
LSP over stdio:

    > cargo install --path plc-lsp   # → ~/.cargo/bin/plc-lsp

It finds the vault the way plc does (PALACE_DIR), falling back to the
editor's workspace root, and indexes every .md note outside hidden
directories.

    definition    on a [[link]]: the note it names; [[note#Heading]] and
                  [[note#^block]] land on that line, [[#Heading]] stays in
                  the current note. A name with no note but declared in
                  .plc/config ([[revolut]], [[food/groceries]]) lands on
                  its declaration.
    references    on a [[link]]: every link to the same target; anywhere
                  else: the current note's backlinks.
    completion    after [[: note names as spelled on disk, plus declared
                  accounts and categories.

A link names a note by its file name: the last path segment, without
|alias, #heading or ^block, compared case-insensitively across Unicode and
regardless of how the file system composed the name — the same rule plc
orphans follows. The index tracks open buffers as you type and, when the
editor supports file watching, files changed outside it.
