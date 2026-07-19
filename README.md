# plc

`plc` is a plain-text note-taking CLI. Every subcommand _creates or resolves a
file and prints its path_ — it never opens an editor and is never interactive;
a thin zsh wrapper opens the printed path with `$EDITOR`. Notes are `.md` files
under a vault (`$PALACE_DIR`), stamped with a header and linked with
`[[wikilinks]]`.

    plc daily      # today's daily note
    plc weekly     # this ISO week's note
    plc shot       # a timestamped snapshot
    plc top        # the vault landing page
    plc fin        # finances (this document)
    plc orphans    # notes with no links in or out
    plc init       # scaffold the vault tree

This README documents **`plc fin`**, a plain-text finance tracker built on the
same store — think of it as the _Debit & Credit_ app's simplicity with
[Ledger](https://ledger-cli.org/)'s plain-text discipline, sharing one clock
and one link graph with your notes.

---

# 1 Fat-free finance

`plc fin` keeps your money in the same place as your prose: one small `.md`
file per day, under the daily tree, marked by a `+ledger` filename postfix and
tagged `[[ledger]]`. A _transaction_ is a single line beginning with `$`. There
are no databases, no sync, no lock files — just text you can read, grep, and
edit by hand.

Under the hood it is **double-entry**: money never appears or disappears, it
only moves from one bucket to another, and every transaction sums to zero. You
never have to write the second side — `plc` infers it — but the guarantee is
always checked, so a report can tell you `book : 0.00 ✓`.

Three kinds of bucket:

- **`@` accounts** — physical places that hold real money (`@[[cash]]`,
  `@[[bnp]]`, `@[[card]]`). Their balances persist and are your net worth.
- **`#` categories** — ephemeral income sources and expense sinks
  (`#[[salary]]`, `#[[coffee]]`, `#[[rent]]`). Where money comes from and
  goes to.
- **`~` tags** — cross-cutting projects/events (`~[[japan-trip]]`), a grouping
  label that is _not_ a money leg.

Accounts, categories, and tags are all `[[wikilinks]]`, so they double as nodes
in your notes' link graph (`plc orphans` sees `cash`, `coffee`, `salary`, …).

---

# 2 Tutorial

Point `plc` at a vault and scaffold it (skip if you already have one):

    $ export PALACE_DIR=~/vault
    $ plc init
    init: ~/vault — 9 created, 0 already present (9 dirs)

Record a few transactions. `plc fin add` formats the line, writes it into
today's ledger, and prints the file's path:

    $ plc fin add 3000 opening balance -a bnp -c opening --income
    ~/vault/notes/management/daily/2026/07/2026-07-19+ledger.md
    $ plc fin add 2400 July pay -a bnp -c salary --income
    ~/vault/notes/management/daily/2026/07/2026-07-19+ledger.md
    $ plc fin add 200 ATM -a bnp --to cash
    ~/vault/notes/management/daily/2026/07/2026-07-19+ledger.md
    $ plc fin add 4.50 Blue Bottle -a cash -c coffee
    ~/vault/notes/management/daily/2026/07/2026-07-19+ledger.md

That leaves one file on disk, seeded with a note header and holding four
transaction lines:

    isg 2026-07-19 09:00:00 +0200

    [[ledger]]

    $ 2026-07-19 09:00:01 +0200 +3000.00 EUR  @[[bnp]] #[[opening]]  opening balance
    $ 2026-07-19 09:00:02 +0200 +2400.00 EUR  @[[bnp]] #[[salary]]  July pay
    $ 2026-07-19 09:00:03 +0200 200.00 EUR  @[[bnp]] > @[[cash]]  ATM
    $ 2026-07-19 09:00:04 +0200 -4.50 EUR  @[[cash]] #[[coffee]]  Blue Bottle

## 2.1 The balance / summary report

    $ plc fin report

      Finance — 4 transaction(s) across 1 ledger file(s)

      EUR
        income   : 5400.00
        expenses : 4.50
        net      : +5395.50
        book     : 0.00  ✓

        by account
          bnp               +5200.00
          cash              +195.50

        by category
          coffee            +4.50
          opening           -3000.00
          salary            -2400.00

Physical accounts show what you hold (`bnp +5200.00`, `cash +195.50`). Income
sources show negative — that is normal double-entry: `salary -2400` means
€2400 was drawn _from_ your employer into your accounts. `book : 0.00 ✓`
confirms every leg cancels.

## 2.2 The register report

`plc fin reg` lists transactions in date order with a running net-worth total
(income `+`, expense `-`, transfers net to zero):

    $ plc fin reg

      Register — 4 transaction(s)

      2026-07-19     +3000.00 EUR     +3000.00  @bnp #opening  opening balance
      2026-07-19     +2400.00 EUR     +5400.00  @bnp #salary  July pay
      2026-07-19       200.00 EUR     +5400.00  @bnp > @cash  ATM
      2026-07-19        -4.50 EUR     +5395.50  @cash #coffee  Blue Bottle

Add a search term to narrow it — `plc fin reg coffee` shows only transactions
touching `coffee`, with a running total of just those.

---

# 3 Keeping the journal

## 3.1 Anatomy of a transaction line

    $ 2026-07-18 09:30:00 +0200 * -4.50 EUR  @[[cash]] #[[coffee]] = 195.50 EUR ~[[trip]]  Blue Bottle
    │ └── timestamp ──────────┘ │ └amt┘ └cur┘ └─account─┘ └category┘ └─assertion─┘ └─tag─┘  └─ memo ─┘
    └ marks the line a transaction   │
                                     └ state: * cleared, ! pending

Every field except the amount and one account is optional. In order:

- **`$`** — a leading `$` (then a space) marks the line as a transaction; any
  other line is prose and is ignored.
- **timestamp** — `YYYY-MM-DD HH:MM:SS ±ZZZZ`, the same format as the note
  stamp line. `plc fin add` stamps _now_ by default; omit it and the
  transaction inherits the ledger file's day.
- **state** — `*` cleared or `!` pending (reconciliation); omitted = uncleared.
- **amount** — a decimal. `-` is an outflow (expense), `+` an inflow (income);
  a transfer uses a bare magnitude.
- **currency** — an optional ISO code; defaults to `$PLC_CURRENCY`, else `EUR`.
  Reports subtotal per currency (there is no FX conversion).
- **`@[[account]]`** — the account (required).
- **`#[[category]]`** for an expense/income, **or** **`> @[[account2]]`** for
  a transfer.
- **`= <balance> [CUR]`** — an optional balance assertion (§5.3).
- **`~[[tag]]`** — zero or more project/event tags (§4.3), nested with `/`.
- **memo** — free text to end of line.

Names are lowercased and may nest with `/`; a `|alias`, `#heading`, or `^block`
suffix is dropped. So `@[[Bank/Checking|joint]]` is stored as `bank/checking`.

## 3.2 Where money comes from

Every transaction is a move between two buckets that nets to zero. You write
one side; `plc` supplies the other.

An **expense** — money leaves an account, lands in a category:

    $ plc fin add 4.50 Blue Bottle -a cash -c coffee
    #  → $ … -4.50 EUR  @[[cash]] #[[coffee]]  Blue Bottle
    #    cash -4.50, coffee +4.50

**Income** — money comes from a source category into an account:

    $ plc fin add 2400 July pay -a bnp -c salary --income
    #  → $ … +2400.00 EUR  @[[bnp]] #[[salary]]  July pay
    #    salary -2400 (drawn from the outside world), bnp +2400

A **transfer** — money moves between two of your own accounts (net worth
unchanged):

    $ plc fin add 200 ATM -a bnp --to cash
    #  → $ … 200.00 EUR  @[[bnp]] > @[[cash]]  ATM
    #    bnp -200, cash +200

When you first start, seed each account's balance with an opening-balance
income from an `opening` (equity) category — that is where your existing money
"comes from":

    $ plc fin add 3000 opening -a bnp -c opening --income

## 3.3 Back-dating and reconciliation

`plc fin add` writes into _today's_ file and stamps _now_. Override the instant
with `--date` (a full timestamp, or a bare `YYYY-MM-DD` = local midnight), and
mark reconciliation state with `--cleared` / `--pending`:

    $ plc fin add 900 rent -a bnp -c rent --date 2026-07-01 --cleared
    #  → $ 2026-07-01 00:00:00 +0200 * -900.00 EUR  @[[bnp]] #[[rent]]  rent

---

# 4 Structuring your finances

## 4.1 Accounts vs. categories

Ask _"is this a place my money actually lives?"_ If yes it is an `@` account
(you could count it); if it is only _what the money was for_, it is a `#`
category. Net worth is the sum of your `@` accounts; the `#` side is your cash
flow.

## 4.2 Hierarchy with `/`

Accounts, categories, and tags nest with `/`, and reports roll children up into
their parent:

    $ plc fin add 60 -a bank/checking -c food/groceries
    $ plc fin add 25 -a bank/checking -c food/dining
    $ plc fin report

        by category
          food              +85.00
            dining          +25.00
            groceries       +60.00

`--depth N` caps the tree; `plc fin report --depth 1` folds the children back
into `food +85.00`.

## 4.3 Projects and events (`~`)

A `~` tag groups spending that cuts across accounts and categories — a trip, a
renovation, a client. It is an attribute, not a money leg, so it never affects
the balance. Add one or more with `-p` (repeatable):

    $ plc fin add 80 ramen -a card -c food -p japan-trip/food
    $ plc fin add 300 hotel -a card -c lodging -p japan-trip/lodging
    $ plc fin report

        by project
          japan-trip        +380.00
            food            +80.00
            lodging         +300.00

---

# 5 Advanced entries

## 5.1 Long lines wrap to a block

The vault is reflowed to 66 columns, so when a transaction would exceed that,
`plc fin add` writes a **block**: a `$` head line plus indented continuation
lines (each ≤ 66), which parse back to the same transaction.

    $ plc fin add 4.50 airport latte before the long flight home \
        -a cash -c coffee -p japan-trip/leisure -p work

    $ 2026-07-19 18:20:00 +0200 -4.50 EUR  @[[cash]] #[[coffee]]
        ~[[japan-trip/leisure]] ~[[work]]
        airport latte before the long flight home

## 5.2 Splitting one payment across categories

Split a single payment with `--split CAT=AMOUNT` (repeatable); the legs must
sum to the total:

    $ plc fin add 90 Costco -a card --split food=60 --split household=25 --split tax=5

    $ 2026-07-19 12:09:45 +0200 -90.00 EUR  @[[card]]
        #[[food]]  -60.00 EUR
        #[[household]]  -25.00 EUR
        #[[tax]]  -5.00 EUR
        Costco

The report distributes the payment across all three categories while the book
still balances.

## 5.3 Balance assertions

Assert an account's balance right after a transaction to catch drift, with
`--assert` (or a `= <balance>` on the line):

    $ plc fin add 4.50 coffee -a cash -c coffee --assert 195.50
    #  → $ … -4.50 EUR  @[[cash]] #[[coffee]] = 195.50 EUR  coffee

`plc fin check` replays every transaction in date order and verifies each
assertion:

    $ plc fin check
      1 balance assertion(s) OK  ✓

    $ plc fin check        # if the books have drifted
    fin: 1 check(s) failed:
      2026-07-19  @cash: expected +999.00 EUR, got +185.50

---

# 6 Reports

    plc fin report [PATTERN…]     summary: net, by account / category / project
    plc fin reg    [PATTERN…]     chronological register + running total
    plc fin check  [--strict]     verify balance assertions (and declarations)

`PATTERN` keeps transactions whose account, category, tag, or memo contains it
(case-insensitive; multiple patterns match if any does). Both `report` and
`reg` share these filters:

    --since YYYY-MM-DD    --until YYYY-MM-DD    --month YYYY-MM
    --cleared             --pending
    --depth N             (report only: cap the tree)

For example, July's dining, cleared only:

    $ plc fin report food/dining --month 2026-07 --cleared

---

# 7 Keeping it consistent (`--strict`)

To catch typos, declare your accounts, categories, and commodities on their own
lines in any ledger file (they are ignored by the transaction parser):

    account cash
    account bnp
    category coffee
    category food/groceries
    commodity EUR

Then `plc fin check --strict` flags anything used but never declared:

    $ plc fin check --strict
    fin: 2 check(s) failed:
      undeclared account: @card
      undeclared category: #food

---

# 8 Command reference

    plc fin                       seed/print today's ledger path (open it yourself)
    plc fin add AMOUNT [MEMO…]    append a transaction (stamps now)
      -a, --account ACCOUNT       the account (required)
      -c, --category CATEGORY     expense/income category
          --to ACCOUNT            transfer destination (instead of a category)
          --split CAT=AMOUNT      split across categories (repeatable; must sum)
      -i, --income                inflow (default is an expense/outflow)
          --cur CUR               currency (default $PLC_CURRENCY, else EUR)
      -p, --project TAG           project/event tag, nested with `/` (repeatable)
      -d, --date WHEN             YYYY-MM-DD or a full timestamp (default: now)
          --cleared / --pending   reconciliation state
          --assert BALANCE        assert the account balance afterwards
    plc fin report [PATTERN…]     summary report        (+ filters, --depth)
    plc fin reg    [PATTERN…]     chronological register (+ filters)
    plc fin check  [--strict]     verify assertions (+ undeclared names)

## Storage

Transactions live in one file per day under the daily tree:

    $PALACE_DIR/notes/management/daily/YYYY/MM/YYYY-MM-DD+ledger.md

Each file is an ordinary note (stamped header + `[[ledger]]` tag) whose body is
transaction lines. `plc` only ever appends; edit the files freely by hand.

## Environment

    PALACE_DIR    the vault root (required)
    PLC_CURRENCY  default currency when a line omits one (default: EUR)
