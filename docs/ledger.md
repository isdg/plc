# plc ledger — plain-text ledger

`plc ledger` is a plain-text ledger tracker built on the same store as your notes
— think of it as the _Debit & Credit_ app's simplicity with
[Ledger](https://ledger-cli.org/)'s plain-text discipline, sharing one clock and
one link graph with your prose. For the rest of `plc`'s commands, see the
[top-level README](../README.md).

---

# 1 Fat-free ledger

`plc ledger` keeps your money in the same place as your prose: one small `.md`
file per day, under the daily tree, marked by a `+ledger` filename postfix and
tagged `[[ledger]]`. A _transaction_ is a single line beginning with `$`. There
are no databases, no sync, no lock files — just text you can read, grep, and
edit by hand.

Under the hood it is **double-entry**: money never appears or disappears, it
only moves from one bucket to another, and every transaction sums to zero. You
never have to write the second side — `plc` infers it — but the guarantee is
always checked, so a report can tell you `book : 0.00 OK`.

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

Record a few transactions. `plc ledger add` formats the line, writes it into
today's ledger, and prints the file's path:

    $ plc ledger add 3000 opening balance -a bnp -c opening --income
    ~/vault/notes/management/daily/2026/07/2026-07-19+ledger.md
    $ plc ledger add 2400 July pay -a bnp -c salary --income
    ~/vault/notes/management/daily/2026/07/2026-07-19+ledger.md
    $ plc ledger add 200 ATM -a bnp --to cash
    ~/vault/notes/management/daily/2026/07/2026-07-19+ledger.md
    $ plc ledger add 4.50 Blue Bottle -a cash -c coffee
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

    $ plc ledger report

      Ledger — 4 transaction(s) across 1 ledger file(s)

      EUR
        income   : 5400.00
        expenses : 4.50
        net      : +5395.50
        book     : 0.00  OK

        by account
          bnp               +5200.00
          cash              +195.50

        by category
          coffee            +4.50
          opening           -3000.00
          salary            -2400.00

Physical accounts show what you hold (`bnp +5200.00`, `cash +195.50`). Income
sources show negative — that is normal double-entry: `salary -2400` means
€2400 was drawn _from_ your employer into your accounts. `book : 0.00 OK`
confirms every leg cancels. Accounts that net to zero (a settled or closed
account) are hidden from `by account`; categories always list in full.

## 2.2 The register report

`plc ledger reg` lists transactions in date order with a running net-worth total
(income `+`, expense `-`, transfers net to zero):

    $ plc ledger reg

      Register — 4 transaction(s)

      2026-07-19     +3000.00 EUR     +3000.00  @bnp #opening  opening balance
      2026-07-19     +2400.00 EUR     +5400.00  @bnp #salary  July pay
      2026-07-19       200.00 EUR     +5400.00  @bnp > @cash  ATM
      2026-07-19        -4.50 EUR     +5395.50  @cash #coffee  Blue Bottle

Add a search term to narrow it — `plc ledger reg coffee` shows only transactions
touching `coffee`, with a running total of just those.

## 2.3 The balance snapshot

`plc ledger balance` (alias `bal`) is a compact "where do I stand" view — net
worth, income/expense/net, non-zero account balances, and the most recent
transactions:

    $ plc ledger bal

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
`plc ledger bal rent -n 3` shows your rent standing plus the last three rent moves.

---

# 3 Keeping the journal

## 3.1 Anatomy of a transaction line

    $ ^09c1bce0826d 2026-07-18 09:30:00 +0200 * -4.50 EUR  @[[cash]] #[[coffee]] = 195.50 EUR ~[[trip]]
    │ └─── id ────┘ └── timestamp ──────────┘ │ └amt┘ └cur┘ └─account─┘ └category┘ └─assertion─┘ └─tag─┘
    └ marks the line a transaction            │
                                              └ state: * cleared, ! pending

Every field except the amount and one account is optional. In order:

- **`$`** — a leading `$` (then a space) marks the line as a transaction; any
  other line is prose and is ignored.
- **`^id`** — a stable 12-hex short hash (git-commit style), a durable handle for
  a single transaction. `plc ledger add` seeds it from the transaction's content
  and then **freezes** it: editing the line later does not change the id. Legacy
  lines have none until `plc doctor` backfills them. See §7.1.
- **timestamp** — `YYYY-MM-DD HH:MM:SS ±ZZZZ`, the same format as the note
  stamp line. `plc ledger add` stamps _now_ by default; omit it and the
  transaction inherits the ledger file's day.
- **state** — `*` cleared or `!` pending (reconciliation); omitted = uncleared.
- **amount** — a decimal. `-` is an outflow (expense), `+` an inflow (income);
  a transfer uses a bare magnitude. On the `add` command line the amount may be
  an arithmetic expression — `plc ledger add '3*4.50+1'` books 14.50 (§5.4).
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

    $ plc ledger add 4.50 Blue Bottle -a cash -c coffee
    #  → -4.50 EUR  @[[cash]] #[[coffee]]     (cash -4.50, coffee +4.50)

**Income** — money comes from a source category into an account:

    $ plc ledger add 2400 July pay -a bnp -c salary --income
    #  → +2400.00 EUR  @[[bnp]] #[[salary]]   (salary -2400 from outside, bnp +2400)

A **transfer** — money moves between two of your own accounts (net worth
unchanged):

    $ plc ledger add 200 ATM -a bnp --to cash
    #  → 200.00 EUR  @[[bnp]] > @[[cash]]     (bnp -200, cash +200)

When you first start, seed each account's balance with an opening-balance
income from an `opening` (equity) category — that is where your existing money
"comes from":

    $ plc ledger add 3000 opening -a bnp -c opening --income

## 3.3 Back-dating and reconciliation

`plc ledger add` writes into the ledger for the transaction's own day (from
`--date`, else today) and stamps _now_ unless told otherwise. Override the
instant with `--date` (a full timestamp, or a bare `YYYY-MM-DD` = local
midnight), and mark reconciliation state with `--cleared` / `--pending`:

    $ plc ledger add 900 rent -a bnp -c rent --date 2026-07-01 --cleared
    #  → $ 2026-07-01 00:00:00 +0200 * -900.00 EUR  @[[bnp]] #[[rent]]

A back-dated entry lands in _its own_ day's file
(`.../2026/07/2026-07-01+ledger.md`), not today's — so bulk history imports
file each transaction where it belongs.

## 3.4 Symbolic shorthand (`-T`)

Instead of `-a`/`-c`/`--to`/`--income`/`--assert`, you can draw the transaction
with a single `-T SPEC`, where the arrow shows which way the money flows:

    $ plc ledger add 5000 pay   -T "revolut <- salary"     # income  (from a category)
    $ plc ledger add 11   lunch -T "revolut -> food/out"   # expense (into a category)
    $ plc ledger add 200  atm   -T "revolut -> cash"       # transfer (to an account)
    $ plc ledger add 0    check -T "revolut = 2300"        # balance assertion

The kind is derived from which side is an **account** vs a **category**:
account → category is an expense, category → account is income, account →
account is a transfer. A name is an account when it is a declared account or
written `@name`, a category when `#name` or a bare undeclared name.

The spec is **associative** — either side may be the account, so these are the
same expense:

    -T "revolut -> taxi"        ==        -T "taxi <- revolut"

`-T` is shorthand for the flag form (`-a revolut -c salary --income`); the two
can't be mixed on one command. Source and destination must differ — a transfer
to the same account is rejected.

---

# 4 Structuring your ledgers

## 4.1 Accounts vs. categories

Ask _"is this a place my money actually lives?"_ If yes it is an `@` account
(you could count it); if it is only _what the money was for_, it is a `#`
category. Net worth is the sum of your `@` accounts; the `#` side is your cash
flow. A debt you owe is just an `@` account with a negative balance.

## 4.2 Hierarchy with `/`

Accounts, categories, and tags nest with `/`, and reports roll children up into
their parent:

    $ plc ledger add 60 -a bank/checking -c food/groceries
    $ plc ledger add 25 -a bank/checking -c food/dining
    $ plc ledger report

        by category
          food              +85.00
            dining          +25.00
            groceries       +60.00

`--depth N` caps the tree; `plc ledger report --depth 1` folds the children back
into `food +85.00`.

## 4.3 Projects and events (`~`)

A `~` tag groups spending that cuts across accounts and categories — a trip, a
renovation, a client. It is an attribute, not a money leg, so it never affects
the balance. Add one or more with `-p` (repeatable):

    $ plc ledger add 80 ramen -a card -c food -p japan-trip/food
    $ plc ledger add 300 hotel -a card -c lodging -p japan-trip/lodging
    $ plc ledger report

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

    $ plc ledger add 11 takos -a revolut -c food/out

    $ 2026-07-19 16:39:34 +0200 -11.00 EUR  @[[revolut]] #[[food/out]]
        takos

Tags stay up on the head with the rest of the accounting:

    $ 2026-07-19 18:20:00 +0200 -4.50 EUR  @[[cash]] #[[coffee]] ~[[japan-trip]]
        airport latte before the long flight home

`plc ledger fmt` re-renders every ledger file into this canonical layout — handy
after bulk edits or an import. It rewrites only files that change; `--check`
reports what would change without touching anything.

## 5.2 Splitting one payment across categories

Split a single payment with `--split CAT=AMOUNT` (repeatable); the legs must
sum to the total:

    $ plc ledger add 90 Costco -a card --split food=60 --split household=25 --split tax=5

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

    $ plc ledger add 4.50 coffee -a cash -c coffee --assert 195.50
    #  → $ … -4.50 EUR  @[[cash]] #[[coffee]] = 195.50 EUR
    #        coffee

For a pure checkpoint that moves no money, add a zero-amount transaction — it
contributes nothing to any balance but is still verified:

    $ plc ledger add 0 balance check -a cash --assert 195.50

`plc ledger check` replays every transaction in date order and verifies each
assertion:

    $ plc ledger check
      1 balance assertion(s) OK

    $ plc ledger check        # if the books have drifted
    ledger: 1 check(s) failed:
      2026-07-19  @cash: expected +999.00 EUR, got +185.50

## 5.4 Arithmetic in the amount

The `AMOUNT` argument (and each `--split` leg) may be an arithmetic expression
— `+ - * / ( )` over decimals, rounded to the nearest cent — so you can total a
receipt or split a bill inline:

    $ plc ledger add '3*4.50+1' lunch -a cash -c food       # → 14.50
    $ plc ledger add 90 shop -a card --split food=60 --split 'house=90-60'

---

# 6 Reports

    plc ledger report  [PATTERN…]    summary: net, by account / category / project
    plc ledger reg     [PATTERN…]    chronological register + running total
    plc ledger balance [PATTERN…]    net worth, account balances, recent (alias bal)
    plc ledger check   [--strict]    verify balance assertions (and declarations)
    plc ledger fmt     [--check]     reformat every ledger file in place

`PATTERN` keeps transactions whose account, category, tag, or memo contains it
(case-insensitive; multiple patterns match if any does). `report`, `reg`, and
`balance` share these filters:

    --since YYYY-MM-DD    --until YYYY-MM-DD    --month YYYY-MM
    --cleared             --pending
    --depth N             (report only: cap the tree)

For example, July's dining, cleared only:

    $ plc ledger report food/dining --month 2026-07 --cleared

`plc ledger stat` brings the `plc stat` calendar heatmap / line-plot to daily
**spend** (see the README's `stat` section; `--of income|net` switches what it
measures).

---

# 7 A declared vocabulary (typo guard)

Declare the accounts and categories you actually use, and `plc ledger add` will
refuse an undeclared one — so `-c cofee` is caught instead of silently creating
a bogus category. Accounts and categories are the same essence (named ledger
buckets), so one command manages both — `--physical` for accounts (`@`),
`--ephemeral` for categories (`#`). Declarations live in `.plc/config` (see §9):

    plc ledger declare                        list every declared account + category
    plc ledger declare cash bnp --physical    declare account(s)
    plc ledger declare coffee   --ephemeral   declare category(ies)
    plc ledger declare bnp --physical -r      remove
    plc ledger declare --import               seed from every name used in ledgers
                                           (add --physical/--ephemeral for one kind)

Once a set is non-empty it is enforced; an unknown name is rejected:

    $ plc ledger add 4.50 latte -a cash -c cofee
    ledger: undeclared name(s) — declare them or pass -n to add now:
      #cofee  (plc ledger declare cofee --ephemeral)

Pass `-n/--new` to declare the name on the fly and add in one go. An empty set
means "not enforced yet", so fresh vaults and bulk imports keep working; run
`--import` once to adopt everything you already use.

A name can't be declared as **both** an account and a category — `declare`
rejects the second, and no single transaction may use the same name on both
legs (`@revolut` with `#revolut`). `plc doctor` flags any pre-existing
clash.

`plc ledger check --strict` reports the same undeclared names across the whole
journal at once (reading `.plc/config` plus any in-file `account NAME` /
`category NAME` / `commodity CODE` directive lines):

    $ plc ledger check --strict
    ledger: 2 check(s) failed:
      undeclared account: @card
      undeclared category: #food

## 7.1 `plc doctor`

`doctor` compares `.plc/config` against the names actually used in your ledgers
and reports what's off, with a repair command for each finding:

    $ plc doctor
      ! 1 categories used but not declared:
          #transport  (plc ledger declare transport --ephemeral)
      ! 1 categories declared but never used (typo/stale?):
          #rent  (plc ledger declare rent --ephemeral -r)
      ! no default currency in .plc/config — ledgers use EUR
      · accounts: guard off (12 used, none declared) — `plc ledger declare --import --physical`

It also backfills any transaction still missing a stable `^id` (§3.1) — an entry
imported or hand-written before ids existed — seeding a frozen git-style hash
onto each, and flags a legacy `.last-do` left at the vault root:

    $ plc doctor
      ! 2 transaction(s) missing a stable id
          assign them: plc doctor --fix (frozen git-style ^id)

`plc doctor --fix` applies the safe repairs — importing undeclared names into an
already-active guard, setting the default currency, assigning the missing ids,
and migrating the pointer into `.plc/` — while leaving judgement calls (an unused
declaration might be a typo *or* a real bucket you've yet to use, and two
transactions sharing an id must be told apart by hand) for you to resolve with
the printed command.

---

# 8 Recent activity, editing, and undo

`.plc/last-transactions` is an always-current cache of your recent transactions,
rebuilt from the ledgers on every `add` / `edit` / `last` / `undo` (self-creating,
so it covers all history — imports and hand-edits included — and never goes stale).

`plc ledger last` shows the most recent transactions, newest first:

    $ plc ledger last -n 3         # the 3 most recent

## 8.1 Editing a transaction by its id

Every transaction carries a stable `^id` (§3.1). `plc ledger edit <ID>` targets one
by that id — a **unique prefix is enough**, exactly like a git short hash. Its
frozen id does **not** change when you edit it, so the handle stays valid.

With no flags it just prints the transaction's `path:line`, so the shell wrapper
can open it in `$EDITOR`:

    $ plc ledger edit 85b4d8
    …/2026/07/2026-07-20+ledger.md:7

With field flags it rewrites the entry in place (same flags as `add`) and prints
the file path:

    $ plc ledger edit 85b4d8 --amount 12.50 --memo "team lunch" --cleared
    $ plc ledger edit 85b4d8 --category food/out          # recategorize
    $ plc ledger edit 85b4d8 --to savings                 # turn it into a transfer

A `--date` that lands on a different day moves the entry into that day's ledger
file. An ambiguous prefix (or an unknown id) is reported rather than guessed; a
split's total can't be changed this way (edit its legs in the file instead).

## 8.2 Deleting a transaction

`plc ledger rm <ID>` deletes a transaction by its id (a unique prefix, git-style)
— the general form of undo, for any transaction rather than only the newest:

    $ plc ledger rm 85b4d8

`plc ledger undo` removes the *most recent* transaction from its ledger and
refreshes the cache. Both locate the entry by re-parsing the file (matching the
id, or the newest transaction's value), so they still work after a `fmt` reflow
or a hand-edit — no fragile exact-text match.

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

    plc ledger                       seed/print today's ledger path (open it yourself)
    plc ledger add AMOUNT [MEMO…]    append a transaction (files it in its day)
      -a, --account ACCOUNT       the account (required unless -T supplies it)
      -T, --txn SPEC              symbolic shape: `A -> B` / `A <- B` / `A = N`
                                  (replaces -a/-c/--to/-i/--assert; see §3.4)
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
    plc ledger edit ID [flags]       edit a txn by its ^id (unique prefix; §8.1)
      (no flags)                  print its path:line for an editor
      --amount / --memo / -a / -c / --to / -i / --expense / --cur /
      -p / --no-projects / -d / --cleared / --pending / --uncleared /
      --assert / --no-assert      change that field in place (id stays frozen)
    plc ledger report  [PATTERN…]    summary report         (+ filters, --depth)
    plc ledger reg     [PATTERN…]    chronological register (+ filters)
    plc ledger balance [PATTERN…]    net-worth snapshot      (+ filters, -n N)
    plc ledger check   [--strict]    verify assertions (+ undeclared names)
    plc ledger fmt     [--check]     reformat every ledger file in place
    plc ledger stat    [PATTERN…]    spend calendar/plot/stats (see README)
    plc ledger declare [NAME…]       list/declare the vocabulary
      --physical                  operate on accounts (@)
      --ephemeral                 operate on categories (#)
      -r, --rm                    remove the named entries
          --import                seed from names already used in ledgers
    plc ledger last  [-n N]          the most recent transactions
    plc ledger undo                  remove the most recent transaction
    plc ledger rm ID                 remove a transaction by its ^id (§8.2)

    plc doctor        [--fix]        vault health check (top-level; §7.1)

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
