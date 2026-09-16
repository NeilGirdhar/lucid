# Strings

## Triple-quoted strings are always dedented

Python's triple-quoted string is exactly the characters between the
quotes, indentation included — a multi-line string written inside an
indented block carries that indentation as literal content:

```python
def transfer():
    return """
    Move money between two accounts.

    Raises if either account does not exist.
    """
```
That function returns `"\n    Move money...\n\n    Raises...\n    "`,
four leading spaces on every content line, because the block itself
sits four spaces into the function. Getting the string a reader
actually wants means calling `textwrap.dedent` or `inspect.cleandoc`
by hand, every time, and remembering to.

Lucid dedents a triple-quoted string automatically: the smallest
leading-whitespace count shared by every line after the first is
stripped from all of them, so what the literal reads as — independent
of how deeply the surrounding code happens to be nested — is what it
means:

```python
def transfer():
    return """
    Move money between two accounts.

    Raises if either account does not exist.
    """
```
returns `"\nMove money between two accounts.\n\nRaises if either
account does not exist.\n"`. Re-indenting the function, or moving the
whole block one level deeper, changes nothing about the string's
value — only the source's own indentation moved, and that was never
part of the content. This applies to every triple-quoted string, not
only ones used as documentation; a string is either short enough that
dedenting has nothing to do, or long enough that the reader wants it
anyway.

## No adjacent string literal concatenation

Python concatenates adjacent string literals at compile time. Lucid rejects
adjacent string literals. Use an explicit concatenation operation when a string
is meant to be joined.

```python
path = "/api/" "users"  # error
path = "/api/" + "users"
```
## Strings are not sequences

Python's `str` satisfies `Sequence[str]` — `isinstance("abc",
Sequence)` is `True` — but it does not keep the contract satisfying
`Sequence` is supposed to promise. `Sequence.__contains__` means "some
element equals this value": `x in seq` should hold exactly when
`s[i] == x` for some index `i`. `str.__contains__` does substring
search instead — `"abc" in "abcdef"` is `True`, even though no single
index `i` has `s[i] == "abc"`; every element of the "sequence" is a
one-character string, and `"abc"` is not one. `index` and `count`
break the same contract the same way, finding and counting substrings
rather than equal elements. `str` does not even define `__reversed__`
— `reversed("abc")` works anyway, but only by falling back to
`__len__`/`__getitem__`, not because `str` actually implements the
method `Sequence` advertises.

So a function asking for a sequence or iterable of strings also
accepts a single string, and nothing about that is a type error, even
though the string doesn't behave like one once inside:

```python
def render_lines(lines: Sequence[str]) -> str:
    return str.join(lines, sep="\n")

render_lines("hello")  # type-checks, returns "h\ne\nl\nl\no"
```
A caller who forgot to wrap a single string in a list gets an object
that type-checks and runs, silently producing garbled output instead
of a caught mistake. The same gap causes a second, well-known bug:
code that recurses over anything `Sequence`-shaped — flattening
nested lists, say — has to special-case `str` explicitly or it
recurses forever, since a one-character string is itself a
`Sequence[str]` whose only element is another one-character string.

Lucid keeps `str` narrower: it is `Container[str]` and `Sized`, but
not `Iterable[str]`, `Collection[str]`, or `Sequence[str]`. Strings do
not provide `__iter__`, so the same mistake is a type error instead of
a silent, wrong result — and `"abc" is Sequence` is simply `False`,
closing the gap Python's `isinstance("abc", Sequence)` leaves open,
a result Python's own community has asked for repeatedly, for exactly
the contract violations above:

```python
def render_lines(lines: Iterable[str]) -> str:
    return str.join(lines, sep="\n")

render_lines("hello")  # error: str is not Iterable[str]
```
None of this stops `str` from indexing directly — `text[0]` still
works, since [indexing and iteration are separate capabilities](indexing.md#no-__getitem__-iteration-fallback)
in Lucid, unlike Python's own legacy fallback where the two were
never really independent. A single character back from a single
index needs no sequence contract at all; it's the *sequence*
operations — iterating, containment as element-equality, `index`,
`count`, `reversed` — that `str` doesn't get for free. Code that wants
those asks for the `chars` property explicitly. `chars` returns a
read-only sequence view, `~Sequence[str]`, that keeps `Sequence`'s
actual contract: containment, `index`, and `count` test for an equal
one-character element, not a substring, and `reversed(chars)` works
because `chars` genuinely implements `__reversed__`, not because of a
fallback:

```python
text: str = "hello"
text[0]    # "h" -- ordinary indexing, no Sequence needed

for ch in text:          # error
    ...

chars: ~Sequence[str] = text.chars
chars[0]
"e" in chars     # true: some element equals "e"
"el" in chars    # false: no element equals "el" -- chars holds one-character str

for ch in text.chars:
    ...
```
`str` keeps its own `in`, `index`, and `count` — substring search and
counting, a real and useful job in its own right, just not the one
`Sequence` promises, and not one that needs `chars` to reach:

```python
"e" in text     # true: substring search
"el" in text    # true: substring search
len(text)
```
## No `%` string formatting

Python has three generations of string formatting that still
coexist: `%`-formatting, `.format()`, and f-strings. Lucid keeps one —
f-strings — and removes `%` as a string operator. `%` stays exactly
what it already is for numbers, ordinary modulo; a string on its left
side is a type error instead of a second formatting mini-language to
learn:

```python
"%s is %d" % (name, age)   # error: % is not a string operator
f"{name} is {age}"
```

## Base-formatted string factories

Python's `bin`, `oct`, and `hex` each convert a number to a
prefixed string in one base. Lucid moves them onto `str` itself, as
named factories — the same pattern [Factory
construction](construction.md#factory-construction) already uses for
`Point.origin()` — rather than three unrelated top-level names for
one job, "build a `str` a particular way":

```python
str.bin(255)  # "0b11111111"
str.oct(255)  # "0o377"
str.hex(255)  # "0xff"
```
The same result is also reachable through an f-string's own
format-spec mini-language (`f"{255:#b}"`), for when the string being
built is more than just the number itself.

## `join` is a named factory, not a method on the separator

Python's `",".join(items)` calls `join` on the separator — the
smallest, least interesting part of the operation — with the thing
actually being joined arriving as the argument, backwards from how the
operation reads: "join these items with this separator" becomes "ask
this separator to join these items." Lucid keeps `join` where every
other "build a `str` a particular way" operation already lives, a
named factory on `str` itself, with the pieces being joined as the
primary argument and the separator named instead of implied by which
object happens to own the method:

```python
str.join(["a", "b", "c"], sep=",")  # "a,b,c"
```

[Collections](collections.md) covers the container types built from
values like these — sets, dicts, and records.
