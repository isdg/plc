# plc fin — plain-text finance

`plc fin` is a plain-text finance tracker built on the same store as your notes
— think of it as the _Debit & Credit_ app's simplicity with
[Ledger](https://ledger-cli.org/)'s plain-text discipline, sharing one clock and
one link graph with your prose. For the rest of `plc`'s commands, see the
[top-level README](../README.md).

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
transactions (the accounting on the head line, the memo indented below):

    isg 2026-07-19 09:00:00 +0200

    [[ledger]]

    $ 2026-07-19 09:00:01 +0200 +3000.00 EUR  @[[bnp]] #[[opening]]
        opening balance
    $ 2026-07-19 09:00:02 +0200 +2400.00 EUR  @[[bnp]] #[[salary]]
        July pay
    $ 2026-07-19 09:00:03 +0200 200.00 EUR  @[[bnp]] > @[[cash]]
        ATM
    $ 2026-07-19 09:00:04 +0200 -4.50 EUR  @[[cash]] #[[coffee]]
        Blue Bottle

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
confirms every leg cancels. Accounts that net to zero (a settled or closed
account) are hidden from `by account`; categories always list in full.

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

## 2.3 The balance snapshot

`plc fin balance` (alias `bal`) is a compact "where do I stand" view — net
worth, income/expense/net, non-zero account balances, and the most recent
transactions:

    $ plc fin bal

      Balance — 4 transaction(s)

      EUR
        net worth : +5395.50
        in +5400.00  out -4.50  net +5395.50

        accounts
          bnp               +5200.00
          cash              +195.50

        last 4
        2026-07-19        -4.50 EUR  @cash #coffee  Blue Bottle
        2026-07-19       200.00 EUR  @bnp > @cash  ATM
        2026-07-19     +2400.00 EUR  @bnp #salary  July pay
        2026-07-19     +3000.00 EUR  @bnp #opening  opening balance

`-n N` sets how many recent transactions to show (default 5); it takes the same
`PATTERN` / `--cleared` / date filters as `report` and `reg`, so
`plc fin bal rent -n 3` shows your rent standing plus the last three rent moves.

---

# 3 Keeping the journal

## 3.1 Anatomy of a transaction line

    $ 2026-07-18 09:30:00 +0200 * -4.50 EUR  @[[cash]] #[[coffee]] = 195.50 EUR ~[[trip]]
    │ └── timestamp ──────────┘ │ └amt┘ └cur┘ └─account─┘ └category┘ └─assertion─┘ └─tag─┘
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
  a transfer uses a bare magnitude. On the `add` command line the amount may be
  an arithmetic expression — `plc fin add '3*4.50+1'` books 14.50 (§5.4).
- **currency** — an optional ISO code; defaults to `$PLC_CURRENCY`, else `EUR`.
  Reports subtotal per currency (there is no FX conversion).
- **`@[[account]]`** — the account (required).
- **`#[[category]]`** for an expense/income, **or** **`> @[[account2]]`** for
  a transfer.
- **`= <balance> [CUR]`** — an optional balance assertion (§5.3).
- **`~[[tag]]`** — zero or more project/event tags (§4.3), nested with `/`.
- **memo** — free text, always rendered on its own indented line below the head
  (§5.1).

Names are lowercased and may nest with `/`; a `|alias`, `#heading`, or `^block`
suffix is dropped. So `@[[Bank/Checking|joint]]` is stored as `bank/checking`.

## 3.2 Where money comes from

Every transaction is a move between two buckets that nets to zero. You write
one side; `plc` supplies the other.

An **expense** — money leaves an account, lands in a category:

    $ plc fin add 4.50 Blue Bottle -a cash -c coffee
    #  → -4.50 EUR  @[[cash]] #[[coffee]]     (cash -4.50, coffee +4.50)

**Income** — money comes from a source category into an account:

    $ plc fin add 2400 July pay -a bnp -c salary --income
    #  → +2400.00 EUR  @[[bnp]] #[[salary]]   (salary -2400 from outside, bnp +2400)

A **transfer** — money moves between two of your own accounts (net worth
unchanged):

    $ plc fin add 200 ATM -a bnp --to cash
    #  → 200.00 EUR  @[[bnp]] > @[[cash]]     (bnp -200, cash +200)

When you first start, seed each account's balance with an opening-balance
income from an `opening` (equity) category — that is where your existing money
"comes from":

    $ plc fin add 3000 opening -a bnp -c opening --income

## 3.3 Back-dating and reconciliation

`plc fin add` writes into the ledger for the transaction's own day (from
`--date`, else today) and stamps _now_ unless told otherwise. Override the
instant with `--date` (a full timestamp, or a bare `YYYY-MM-DD` = local
midnight), and mark reconciliation state with `--cleared` / `--pending`:

    $ plc fin add 900 rent -a bnp -c rent --date 2026-07-01 --cleared
    #  → $ 2026-07-01 00:00:00 +0200 * -900.00 EUR  @[[bnp]] #[[rent]]

A back-dated entry lands in _its own_ day's file
(`.../2026/07/2026-07-01+ledger.md`), not today's — so bulk history imports
file each transaction where it belongs.

---

# 4 Structuring your finances

## 4.1 Accounts vs. categories

Ask _"is this a place my money actually lives?"_ If yes it is an `@` account
(you could count it); if it is only _what the money was for_, it is a `#`
category. Net worth is the sum of your `@` accounts; the `#` side is your cash
flow. A debt you owe is just an `@` account with a negative balance.

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

## 5.1 The memo sits on its own line

The `$` head line carries the whole **accounting** — date, amount, account,
category (or transfer destination), balance assertion, and `~` tags — always
together on one line, so `@account` and `#category` never separate. The **memo**
always drops to its own indented line below; a long memo wraps at 79 columns.
The vault is reflowed to 79 columns, but `+ledger` files are excluded from
reflow, so the accounting head can run long when it has to.

    $ plc fin add 11 takos -a revolut -c food/out

    $ 2026-07-19 16:39:34 +0200 -11.00 EUR  @[[revolut]] #[[food/out]]
        takos

Tags stay up on the head with the rest of the accounting:

    $ 2026-07-19 18:20:00 +0200 -4.50 EUR  @[[cash]] #[[coffee]] ~[[japan-trip]]
        airport latte before the long flight home

`plc fin fmt` re-renders every ledger file into this canonical layout — handy
after bulk edits or an import. It rewrites only files that change; `--check`
reports what would change without touching anything.

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
    #  → $ … -4.50 EUR  @[[cash]] #[[coffee]] = 195.50 EUR
    #        coffee

For a pure checkpoint that moves no money, add a zero-amount transaction — it
contributes nothing to any balance but is still verified:

    $ plc fin add 0 balance check -a cash --assert 195.50

`plc fin check` replays every transaction in date order and verifies each
assertion:

    $ plc fin check
      1 balance assertion(s) OK  ✓

    $ plc fin check        # if the books have drifted
    fin: 1 check(s) failed:
      2026-07-19  @cash: expected +999.00 EUR, got +185.50

## 5.4 Arithmetic in the amount

The `AMOUNT` argument (and each `--split` leg) may be an arithmetic expression
— `+ - * / ( )` over decimals, rounded to the nearest cent — so you can total a
receipt or split a bill inline:

    $ plc fin add '3*4.50+1' lunch -a cash -c food       # → 14.50
    $ plc fin add 90 shop -a card --split food=60 --split 'house=90-60'

---

# 6 Reports

    plc fin report  [PATTERN…]    summary: net, by account / category / project
    plc fin reg     [PATTERN…]    chronological register + running total
    plc fin balance [PATTERN…]    net worth, account balances, recent (alias bal)
    plc fin check   [--strict]    verify balance assertions (and declarations)
    plc fin fmt     [--check]     reformat every ledger file in place

`PATTERN` keeps transactions whose account, category, tag, or memo contains it
(case-insensitive; multiple patterns match if any does). `report`, `reg`, and
`balance` share these filters:

    --since YYYY-MM-DD    --until YYYY-MM-DD    --month YYYY-MM
    --cleared             --pending
    --depth N             (report only: cap the tree)

For example, July's dining, cleared only:

    $ plc fin report food/dining --month 2026-07 --cleared

`plc fin stat` brings the `plc stat` calendar heatmap / line-plot to daily
**spend** (see the README's `stat` section; `--of income|net` switches what it
measures).

---

# 7 A declared vocabulary (typo guard)

Declare the accounts and categories you actually use, and `plc fin add` will
refuse an undeclared one — so `-c cofee` is caught instead of silently creating
a bogus category. Declarations live in `.plc/config` (see §9) and are managed
with two mirror commands:

    plc fin acct                 list declared accounts   (alias: accts)
    plc fin acct -a cash bnp     declare account(s)
    plc fin acct -r bnp          remove account(s)
    plc fin acct --import        seed from every account already used in ledgers
    plc fin cat  …               the same, for categories  (alias: cats)

Once a set is non-empty it is enforced; an unknown name is rejected:

    $ plc fin add 4.50 latte -a cash -c cofee
    fin: undeclared name(s) — declare them or pass -n to add now:
      #cofee  (plc fin cat -a cofee)

Pass `-n/--new` to declare the name on the fly and add in one go. An empty set
means "not enforced yet", so fresh vaults and bulk imports keep working; run
`--import` once to adopt everything you already use.

`plc fin check --strict` reports the same undeclared names across the whole
journal at once (reading `.plc/config` plus any in-file `account NAME` /
`category NAME` / `commodity CODE` directive lines):

    $ plc fin check --strict
    fin: 2 check(s) failed:
      undeclared account: @card
      undeclared category: #food

---

# 8 Recent log and undo

Every `plc fin add` is recorded to `.plc/last-transactions`. Review the tail and
reverse a mistaken add:

    $ plc fin last -n 3         # the 3 most recent, newest first
    $ plc fin undo              # remove the last add from its ledger + the log

`undo` only removes the entry when it is still the trailing block of its file —
if you have edited the ledger since, it refuses rather than guess.

---

# 9 Settings (`.plc/config`)

Per-vault settings live in a plain-text file at `<PALACE_DIR>/.plc/config`,
hand-editable or managed by the `acct`/`cat` commands:

    # plc settings
    currency = EUR

    [categories]
    food/groceries
    rent

    [accounts]
    revolut
    cash

`currency` is the vault default when a transaction omits one; the full
precedence is `--cur` > `$PLC_CURRENCY` > `.plc/config` > `EUR`. The
`[categories]` / `[accounts]` sections are the declared vocabulary from §7.

---

# 10 Command reference

    plc fin                       seed/print today's ledger path (open it yourself)
    plc fin add AMOUNT [MEMO…]    append a transaction (files it in its day)
      -a, --account ACCOUNT       the account (required)
      -c, --category CATEGORY     expense/income category
          --to ACCOUNT            transfer destination (instead of a category)
          --split CAT=AMOUNT      split across categories (repeatable; must sum)
      -i, --income                inflow (default is an expense/outflow)
      -n, --new                   declare any new account/category used here
          --cur CUR               currency (default: see §9)
      -p, --project TAG           project/event tag, nested with `/` (repeatable)
      -d, --date WHEN             YYYY-MM-DD or a full timestamp (default: now)
          --cleared / --pending   reconciliation state
          --assert BALANCE        assert the account balance afterwards
      (AMOUNT may be an arithmetic expression — §5.4)
    plc fin report  [PATTERN…]    summary report         (+ filters, --depth)
    plc fin reg     [PATTERN…]    chronological register (+ filters)
    plc fin balance [PATTERN…]    net-worth snapshot      (+ filters, -n N)
    plc fin check   [--strict]    verify assertions (+ undeclared names)
    plc fin fmt     [--check]     reformat every ledger file in place
    plc fin stat    [PATTERN…]    spend calendar/plot/stats (see README)
    plc fin acct  [-a|-r NAME…|--import]   declare/list accounts   (alias accts)
    plc fin cat   [-a|-r NAME…|--import]   declare/list categories (alias cats)
    plc fin last  [-n N]          the most recent transactions
    plc fin undo                  remove the last added transaction

## Storage

Transactions live in one file per day under the daily tree:

    $PALACE_DIR/notes/management/daily/YYYY/MM/YYYY-MM-DD+ledger.md

Each file is an ordinary note (stamped header + `[[ledger]]` tag) whose body is
transaction lines. `plc` only ever appends; edit the files freely by hand.
Settings and state (the config, the recent-transaction log) live under
`<PALACE_DIR>/.plc/`.

## Environment

    PALACE_DIR    the vault root (required)
    PLC_CURRENCY  default currency, overriding `.plc/config` (default: EUR)
