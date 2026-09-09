# Strings

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
    return "\n".join(lines)

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
    return "\n".join(lines)

render_lines("hello")  # error: str is not Iterable[str]
```
Code that wants a character sequence asks for the `chars` property
explicitly. `chars` returns a read-only sequence view, `~Sequence[str]`,
that keeps `Sequence`'s actual contract: containment, `index`, and
`count` test for an equal one-character element, not a substring, and
`reversed(chars)` works because `chars` genuinely implements
`__reversed__`, not because of a fallback:

```python
text: str = "hello"

for ch in text:          # error
    ...

chars: ~Sequence[str] = text.chars
chars[0]
"e" in chars     # True: some element equals "e"
"el" in chars    # False: no element equals "el" -- chars holds one-character str

for ch in text.chars:
    ...
```
`str` keeps its own `in`, `index`, and `count` — substring search and
counting, a real and useful job in its own right, just not the one
`Sequence` promises, and not one that needs `chars` to reach:

```python
"e" in text     # True: substring search
"el" in text    # True: substring search
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
prefixed string in one base. Lucid moves them onto `Str` itself, as
named factories — the same pattern [Factory
construction](construction.md#factory-construction) already uses for
`Point.origin()` — rather than three unrelated top-level names for
one job, "build a `Str` a particular way":

```python
Str.bin(255)  # "0b11111111"
Str.oct(255)  # "0o377"
Str.hex(255)  # "0xff"
```
The same result is also reachable through an f-string's own
format-spec mini-language (`f"{255:#b}"`), for when the string being
built is more than just the number itself.

[Collections](collections.md) covers the container types built from
values like these — sets, dicts, and records.
