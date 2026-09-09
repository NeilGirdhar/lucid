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

Python's `str` satisfies `Sequence[str]`, since a string is itself a
sequence of one-character strings. So a function asking for a sequence or
iterable of strings also accepts a single string, and nothing about that is
a type error:

```python
def render_lines(lines: Sequence[str]) -> str:
    return "\n".join(lines)

render_lines("hello")  # type-checks, returns "h\ne\nl\nl\no"
```
A caller who forgot to wrap a single string in a list gets an object that
type-checks and runs, silently producing garbled output instead of a caught
mistake. Lucid keeps `str` narrower: it is `Container[str]` and
`Sized`, but not `Iterable[str]`, `Collection[str]`, or
`Sequence[str]`. Strings do not provide `__iter__`, so the same mistake
is a type error instead of a silent, wrong result:

```python
def render_lines(lines: Iterable[str]) -> str:
    return "\n".join(lines)

render_lines("hello")  # error: str is not Iterable[str]
```
Code that wants a character sequence asks for the `chars` property explicitly.
`chars` returns a read-only sequence view, `~Sequence[str]`:

```python
text: str = "hello"

for ch in text:          # error
    ...

chars: ~Sequence[str] = text.chars
chars[0]

for ch in text.chars:
    ...
```
String containment and length do not require string iteration:

```python
"e" in text
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
