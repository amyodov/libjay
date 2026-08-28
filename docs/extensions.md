# Non-standard extensions

Everything on this page is a departure from what the reference
implementations answer. Nothing here is on unless a host asks for it by
name, and nothing here is ever recorded against the oracle corpus: the
corpus is what jconsole and GNU APL say, and an extension says something
else on purpose. Flagged behaviour is pinned by hand in
`crates/libjay/tests/extensions.rs` and `python/tests/test_extensions.py`.

## An extension is not a dialect

A **dialect setting** chooses between readings that reference
implementations disagree about — GNU APL's `⍳` against Dyalog's, `⎕IO`, the
comparison tolerance. Every arm of one is somebody's specification, libjay
ships the arm its oracle verifies, and a host may select the other. Dialect
settings live on `Dialect` and are documented in
[coverage.md](coverage.md#which-apl).

An **extension** has no such defence. Switching one on makes libjay answer
something no reference does. That is a legitimate thing to want — text as
characters rather than bytes, one day a math kernel that trades exact
agreement for speed — but it is not the language, so it is opt-in, it is
named, and it is kept in this one place.

## Names

| Scope | Environment variable | Example |
|---|---|---|
| One language | `LIBJAY_{LANG}_*` | `LIBJAY_J_UNICODE_STRINGS` |
| The whole system or the IR | `LIBJAY_*` | reserved; there are none yet |

The short name a program uses is the variable without the `LIBJAY_` prefix,
lowercased: `j_unicode_strings`. Both spellings are accepted everywhere a
name is taken, in any case.

## Where the set comes from

Flags combine (`|` in Rust and C, a list of names in Python and on the
command line), and the set a program compiles under follows the cascade
every other setting follows:

1. **The environment** names the process default, read once. A flag is on
   where its variable is set to `1`, `true`, `yes` or `on`; unset, empty or
   anything else leaves it off.
2. **The compiler** overrides that: `Dialect::extensions`, Python's
   `extensions=`, the CLI's `--extension`, the C ABI's `jay_compile_ext`.
   Naming any set — including none at all — replaces the environment's
   default rather than adding to it.

The override exists so that a library which embeds libjay is never at the
mercy of the environment its host process happens to carry. A program
compiled with `extensions` named answers the same way on every machine.

```rust
use jay::{compile, Dialect, Extensions, Lang};

let dialect = Dialect {
    extensions: Some(Extensions::J_UNICODE_STRINGS),
    ..Dialect::j()
};
let program = compile(Lang::J, "# 'héllo'", &dialect)?;   // 5, not 6
```

```python
import jay
from jay.lang import J

jay.j("# 'héllo'")                                   # 6 — the language
jay.j("# 'héllo'", extensions="j_unicode_strings")   # 5 — the extension
compiler = J.create_compiler(extensions=["j_unicode_strings"])
compiler.compile("# 'héllo'")()                      # 5, for every program
```

```console
$ libjay -e "# 'héllo'"
6
$ libjay -e "# 'héllo'" --extension j_unicode_strings
5
```

```c
jay_program *p = jay_compile_ext(source, "j", -1,
                                 JAY_EXT_J_UNICODE_STRINGS, &err);
```

`jay_compile` — the entry that was always there — takes the process
default. `jay_extension_bit("j_unicode_strings")` turns a name into a bit,
and a bit this build does not have is a refusal rather than a silent no-op.

## The flags

### `j_unicode_strings` — a J literal holds characters, not bytes

`LIBJAY_J_UNICODE_STRINGS`

**What J does.** J's `literal` type is one byte per item, and a quoted
literal holds the UTF-8 bytes of the text it was written with. So `# 'é'` is
2, `# '日本'` is 6, `a. i. 'é'` is `195 169`, `2 3 $ 'héllo!'` cuts a
character in half, and the session display writes those bytes out again —
which is why the text still looks like what was typed. `u:` is how a
sentence asks for wide characters. libjay follows all of this; the corpus
theme `corpus/j/literals.txt` is the evidence.

**What the flag does.** A quoted literal holds one item per character
instead. `# 'é'` is 1, `# '日本'` is 2, `3 u: 'é'` is `233`, a reshape cuts
between characters, and indexing never lands in the middle of one.
Everything else is unchanged: the display is the same text, `a.` is still
the 256 bytes, and APL — Unicode-native, and right as it stands — is not
affected at all.

**When to want it.** Text that is Unicode throughout and is being counted,
indexed or reshaped as text. A J program written against the reference will
be wrong under it, which is why it is off.

**What it does not cover.** The flag governs quoted literals in the source.
Data a host binds (`{name}`) arrives as the characters it was given, under
either reading.

## The residual divergence

libjay has one character type where J has three (one, two and four bytes per
item), and the wider two differ from the narrow one only in how they are
written out — the items and their codes are the same. libjay writes a
character below 256 as that byte, so a literal widened by `u:` shows the
text it was written as rather than the two characters its bytes name, and a
character `u:` made from a code between 128 and 255 shows as a byte no text
can hold. Everything else about a wide character agrees, and every code at
256 or above — the ones that need the wide type to exist at all — displays
alike. The four sentences this costs are pinned in
`corpus/j/divergences.txt`.
